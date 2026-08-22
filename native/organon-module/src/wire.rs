//! **The bytes, at fixed offsets — and the two words that must never move.**
//!
//! Everything here is a plain little-endian integer at a hand-written offset. No `serde`,
//! no `bytemuck`, no `#[repr(C)]` struct read whole. The reason is in the crate manifest and
//! it is worth repeating where the offsets live: the two parties are **separately built
//! binaries**, and a derived encoding makes the wire's stability a property of a dependency
//! version they cannot agree on by construction.
//!
//! # 🚨 Versioning: two words at offsets 0 and 8, for ever
//!
//! [`MAGIC`] at byte 0 and [`WIRE_VERSION`] at byte 8 are the only two fields whose position
//! is a permanent commitment. Everything after them is version-dependent and may be
//! reorganised freely by a future [`WIRE_VERSION`].
//!
//! That split is the whole versioning design, and it exists so that **a mismatch is a
//! legible refusal rather than a garbled picture**. A consumer that could only discover a
//! version disagreement by mis-reading a field would paint something — and a wrong picture
//! is worse than a sentence, which is `region.rs`'s standing rule and §4.6's.
//!
//! ⚠️ **So a version bump is not a compatibility break, it is a compatibility STATEMENT.**
//! Bump it whenever any offset, any field meaning, or any enum tag below changes. Two builds
//! that disagree then refuse each other by name ([`WireFault::VersionMismatch`]), naming both
//! numbers, which is a thing a person can act on.
//!
//! # Why the geometry is in the header rather than agreed out of band
//!
//! [`Header`] carries `slots`, `slot_stride`, the four block offsets and the capacity. A
//! consumer could compute all of it from the same constants the producer used — and that is
//! precisely the failure: two computations of one fact, in two binaries, with no way to
//! notice they disagree. The creator writes the arithmetic down; the opener **validates it
//! against the file it actually mapped** ([`Header::read`]) and refuses a file whose own
//! description does not fit inside itself.

use std::fmt;

/// Byte 0..8, for ever. Chosen to be recognisable in a hex dump and impossible to produce by
/// accident.
pub const MAGIC: [u8; 8] = *b"ORGONMOD";

/// Byte 8..12, for ever. See the module docs: bump on **any** change to an offset, a field
/// meaning, or an enum tag below.
pub const WIRE_VERSION: u32 = 1;

/// The fixed-size header, and the offset every other block is measured from.
pub const HEADER_BYTES: usize = 128;

/// How many frame slots a channel holds.
///
/// 🚨 **Three, and the third one is the argument.** A producer must never write into the slot
/// the consumer is currently reading, and must never write into the one it has just published
/// (or a consumer arriving a microsecond later gets a half-written "newest" frame). Two slots
/// leaves the producer a choice of zero whenever the consumer holds the one that is not the
/// latest — so it either stalls on the consumer's upload or overwrites it. Three leaves
/// exactly one free slot in the worst case, always, with no stall and no arithmetic to get
/// wrong.
///
/// ⚠️ It is a constant rather than a parameter *and it is still written into the file*. A
/// capacity nobody records is a capacity two builds can disagree about silently.
pub const SLOTS: u32 = 3;

/// Every row a producer writes is padded up to this.
///
/// It is `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256) written out by hand, because this crate
/// must not depend on `wgpu` (see the manifest). `gpu::tests::the_row_alignment_is_wgpus` pins
/// the two numbers equal whenever the `wgpu` feature is on — the constant is duplicated, the
/// *agreement* is tested.
///
/// # 🚨 Padded, not tight — and the reason, because the rule alone loses the argument
///
/// The obvious alternative is a **tightly packed** row of `width * bpp` bytes, on the
/// reasoning that then no transport has to know about `COPY_BYTES_PER_ROW_ALIGNMENT`. It
/// costs strictly more work on **both** sides: a producer whose frame came off the GPU
/// through `copy_texture_to_buffer` has padded rows in hand and must repack them to strip the
/// padding, and the console must then re-pad to upload. Matching the alignment means the
/// producer's staging layout **is** the ring's layout, so the copy out is one `memcpy` per row
/// and the console's `write_texture` sends the rows up as they lie.
///
/// ⚠️ **And the disagreement is invisible at every width anyone would test.** At 640, 1280 and
/// 1920, `width * 4` is *already* 256-aligned, so a tight producer and a padded consumer agree
/// **by accident** and every test either side would naturally write passes. It breaks at the
/// first width that is not — 900 wide is 3600 tight against 3840 padded — and the symptom is a
/// **sheared picture**, not an error, because every byte is a valid pixel and only the row
/// boundaries have moved. That is why `FrameCapacity::max_row_stride` is pinned at an
/// unaligned width in `wire`'s tests and why the channel suite carries a 900-wide round trip:
/// a sweep of nice widths exercises the padding path zero times while looking complete.
pub const ROW_ALIGNMENT: u32 = 256;

/// Per-slot header, ahead of the pixels.
pub const SLOT_HEADER_BYTES: usize = 64;

/// The sentinel the header and the status block use for "no slot".
pub const NO_SLOT: u32 = u32::MAX;

// ---------------------------------------------------------------------------------------
// Block offsets. Everything below byte 128 is version-dependent — see the module docs.
// ---------------------------------------------------------------------------------------

pub(crate) const OFF_MAGIC: usize = 0x00;
pub(crate) const OFF_WIRE_VERSION: usize = 0x08;
pub(crate) const OFF_HEADER_BYTES: usize = 0x0C;
pub(crate) const OFF_SLOTS: usize = 0x10;
pub(crate) const OFF_FORMAT: usize = 0x14;
pub(crate) const OFF_MAX_WIDTH: usize = 0x18;
pub(crate) const OFF_MAX_HEIGHT: usize = 0x1C;
pub(crate) const OFF_SLOT_STRIDE: usize = 0x20;
pub(crate) const OFF_CONTROL_OFF: usize = 0x28;
pub(crate) const OFF_STATUS_OFF: usize = 0x30;
pub(crate) const OFF_INPUT_OFF: usize = 0x38;
pub(crate) const OFF_SLOTS_OFF: usize = 0x40;
pub(crate) const OFF_INPUT_CAPACITY: usize = 0x48;

/// The control block — **the console writes, the producer reads, and never the reverse.**
///
/// Every word a module could use to grant itself something lives here, which is the point:
/// §3.1's rule that a manifest cannot grant itself, applied to the running channel. There is
/// no API in this crate through which a producer can write a control word.
pub mod control {
    pub const BYTES: usize = 64;
    /// 🚨 **Where the seqlock's body ENDS, and it is not `BYTES`.**
    ///
    /// A seqlock brackets ordinary loads and stores; the atomics in this block are *not*
    /// ordinary and must never be copied as part of the body. Reading them as plain bytes
    /// would be a data race on the very words the other party stores to atomically — the
    /// same mistake `organon-core::ipc`'s `Reader` avoids by starting its body copy at byte
    /// 4 rather than 0, and for the same reason. So the body is `SEQ + 4 .. BODY_END`, every
    /// atomic lives at or after `BODY_END`, and `wire::tests` pins that.
    pub const BODY_END: usize = 0x20;
    /// Seqlock counter; odd while the console is mid-write. Outside the body it guards.
    pub const SEQ: usize = 0x00;
    /// [`super::Lifecycle`] as `u32`.
    pub const LIFECYCLE: usize = 0x04;
    pub const REQ_WIDTH: usize = 0x08;
    pub const REQ_HEIGHT: usize = 0x0C;
    pub const SIZE_EPOCH: usize = 0x10;
    /// 1 = the console has asked the producer to exit.
    pub const CLOSE: usize = 0x18;
    /// The console's own heartbeat, so a producer that outlives its host can notice and go.
    /// Atomic, outside the seqlock body.
    pub const CONSOLE_ALIVE: usize = 0x20;
    /// The slot the consumer currently holds, or [`super::NO_SLOT`]. Atomic, outside the
    /// seqlock body — the producer reads it on every frame and must never see a stale one.
    /// It lives in the CONTROL block rather than beside `latest_slot` in the status block
    /// because the console writes it, and one writer per block is what makes the ownership
    /// legible at a glance.
    pub const READER_HOLD: usize = 0x30;
}

/// The status block — **the producer writes, the console reads, and never the reverse.**
pub mod status {
    pub const BYTES: usize = 64;
    /// Where the seqlock's body ends. See [`super::control::BODY_END`] — same rule, and the
    /// two are deliberately allowed to differ, because the two blocks carry different fields
    /// and pinning them equal would be a coincidence maintained by hand.
    pub const BODY_END: usize = 0x28;
    /// Seqlock counter; odd while the producer is mid-write.
    pub const SEQ: usize = 0x00;
    /// [`super::ProducerState`] as `u32`.
    pub const STATE: usize = 0x04;
    pub const DRAWN_WIDTH: usize = 0x08;
    pub const DRAWN_HEIGHT: usize = 0x0C;
    pub const FRAMES: usize = 0x10;
    /// The `SIZE_EPOCH` the producer has actually adopted. Lets the console answer "has the
    /// resize landed?" without inferring it from dimensions that may coincide.
    pub const APPLIED_SIZE_EPOCH: usize = 0x18;
    /// [`super::RefusalReason`] as `u32`. Meaningful only while [`STATE`] is
    /// [`super::ProducerState::Refusing`].
    pub const REFUSAL: usize = 0x20;
    /// 🚨 **The liveness counter, and it is NOT the frame counter.** Bumped once per producer
    /// loop iteration whether or not a frame was published, which is what lets a consumer
    /// tell *paused* from *hung*: a paused producer still loops. Atomic, outside the seqlock
    /// body.
    pub const ALIVE: usize = 0x28;
    /// The newest COMMITTED slot, or [`super::NO_SLOT`]. Atomic, outside the seqlock body,
    /// and stamped only after that slot's own seqlock closed.
    pub const LATEST_SLOT: usize = 0x30;
}

/// The console to producer input ring's header. Records follow at `input_off + HEADER_BYTES`.
pub mod input_ring {
    pub const HEADER_BYTES: usize = 32;
    /// Written by the console.
    pub const HEAD: usize = 0x00;
    /// Written by the producer.
    pub const TAIL: usize = 0x04;
    /// Set by the console when a push found the ring full; cleared by the producer once it
    /// has recovered. See [`crate::input`] — the recovery is a `ReleaseAll`, not a shrug.
    pub const OVERFLOW: usize = 0x08;
    /// Bytes per record. Fixed, so head and tail are indices rather than byte cursors.
    pub const RECORD_BYTES: usize = 16;
    /// How many records a channel's ring holds.
    ///
    /// 256 is about four seconds of a fast typist and about one second of 250 Hz pointer
    /// motion. It is not chosen to be un-overflowable — nothing is — it is chosen so that
    /// overflow means *the producer has stopped reading*, which is a real condition with a
    /// real recovery, rather than *a busy moment*, which would train everyone to ignore it.
    pub const CAPACITY: u32 = 256;
}

/// The per-slot header's field offsets, relative to the slot's own start.
pub mod slot {
    /// Seqlock counter; **odd while a frame is being written**. A producer that dies
    /// mid-frame leaves it odd for ever, and the consumer's rule — never read an odd slot —
    /// is what makes that invisible rather than torn.
    pub const SEQ: usize = 0x00;
    pub const WIDTH: usize = 0x04;
    pub const HEIGHT: usize = 0x08;
    pub const FORMAT: usize = 0x0C;
    pub const ROW_STRIDE: usize = 0x10;
    pub const FRAME_INDEX: usize = 0x18;
    /// 🚨 **The instrument for §4.4's missing number**, and eight bytes is the whole cost.
    /// `SystemTime::now()` at publish, in nanoseconds since the Unix epoch — a clock the two
    /// processes genuinely share, unlike `Instant`, which has no cross-process meaning at
    /// all. It does not measure staleness; it makes staleness *measurable* the moment two
    /// processes exist, which is the thing the design says cannot be estimated.
    ///
    /// ⚠️ `SystemTime` is not monotonic. A clock adjustment mid-run yields an age that is
    /// negative or absurd, and `FrameView::age` answers `None` for both rather than reporting
    /// a number it cannot stand behind.
    pub const PUBLISHED_UNIX_NANOS: usize = 0x20;
    /// The control block's `SIZE_EPOCH` this frame was drawn under.
    pub const SIZE_EPOCH: usize = 0x28;
}

/// Round `v` up to a multiple of `to`. `to` must be non-zero.
pub(crate) const fn align_up(v: u64, to: u64) -> u64 {
    v.div_ceil(to) * to
}

// ---------------------------------------------------------------------------------------
// The closed vocabularies that cross the wire
// ---------------------------------------------------------------------------------------

/// What a pixel is, as a **stable number this crate owns**.
///
/// 🚨 Deliberately not `wgpu::TextureFormat`. Three reasons, in order of how expensive they
/// are to discover later:
///
/// 1. A wgpu type in the wire format ties two independently-built binaries to one wgpu
///    version, at which point "the protocol" is really "the same `Cargo.lock` on both sides"
///    — which is the one thing a hosted module is supposed to make unnecessary.
/// 2. `TextureFormat` has no stable numeric representation. Its variants are ordered by
///    whatever the enum's source order happens to be, so `as u32` is a number that changes
///    when upstream inserts a variant — silently, in a direction that reads as corruption.
/// 3. It has around a hundred variants and this protocol supports two. A type whose values
///    are mostly unrepresentable is a type that promises what it cannot deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PixelFormat {
    /// `console_main.rs`'s `BACKDROP_FORMAT`, and the format T0 measured. The default.
    Rgba8UnormSrgb = 1,
    /// The same bytes in the other order — what a D3D12 swapchain hands back without a
    /// conversion pass. Present so a producer on that path is not forced to swizzle; nothing
    /// in this crate converts between the two.
    Bgra8UnormSrgb = 2,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            PixelFormat::Rgba8UnormSrgb | PixelFormat::Bgra8UnormSrgb => 4,
        }
    }

    pub const fn from_wire(v: u32) -> Option<PixelFormat> {
        match v {
            1 => Some(PixelFormat::Rgba8UnormSrgb),
            2 => Some(PixelFormat::Bgra8UnormSrgb),
            _ => None,
        }
    }

    pub const fn to_wire(self) -> u32 {
        self as u32
    }
}

/// 🚨 **`Attached` is what a module gets on arrival, always.**
///
/// `CLAUDE.md` invariant 4 — *new capability defaults to inert* — arriving where §5.1 puts
/// it: the lifecycle belongs to the **protocol**, not to the module and not to the viewport.
/// A module that could declare itself auto-running would be a manifest granting itself
/// something, which §3.1 forbids.
///
/// It is structural rather than a convention: [`Lifecycle::default`] is `Attached`,
/// [`crate::ModuleChannel::create`] writes it, and the producer half of this crate exposes no
/// way to change it. Only the console writes the control block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum Lifecycle {
    /// The producer exists and is drawing, but **time is not advancing**. §5.1's paused start
    /// state, and the reason `Attached` reads as deliberate rather than broken: a paused
    /// picture that does not eat your keystrokes.
    #[default]
    Attached = 0,
    /// Ticking. Reached only because a person clicked into the viewport (§5.3).
    Running = 1,
}

impl Lifecycle {
    pub const fn from_wire(v: u32) -> Option<Lifecycle> {
        match v {
            0 => Some(Lifecycle::Attached),
            1 => Some(Lifecycle::Running),
            _ => None,
        }
    }
    pub const fn to_wire(self) -> u32 {
        self as u32
    }
}

/// What the producer says about itself. The console never writes this.
///
/// ⚠️ It is a **claim**, not an observation, and the difference is the whole of §4.6's "dead
/// versus slow" question — see [`crate::Presence`], which is what the console acts on and
/// which weighs this against the liveness counter and a clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum ProducerState {
    /// Opened the channel; has not published a frame yet.
    #[default]
    Starting = 0,
    /// Has published at least one frame and intends to publish more.
    Producing = 1,
    /// Alive and looping, deliberately not advancing time — `Lifecycle::Attached` honoured.
    Paused = 2,
    /// 📌 **A clean farewell, and it is the cheapest thing in this protocol.** Without it, a
    /// module that quit on purpose is indistinguishable from one that hung, and the console
    /// would spend the whole death timeout painting "starting…" over a deliberate exit.
    Gone = 3,
    /// 🚨 **Alive, looping, and unable to produce — the state that would otherwise read as
    /// healthy.**
    ///
    /// A producer that declines every frame bumps its liveness counter exactly like a paused
    /// one, so without this word the console concludes *`Paused`* — which is the **arrival
    /// state**, and therefore the least alarming thing it could possibly decide. §4.6 requires
    /// *"stopped producing"* to be sayable, and this is precisely the case that produces it.
    ///
    /// ⚠️ It is a **claim**, and a claim is not the mechanism. `Presence::judge` also measures
    /// frame silence against the clock, so a producer that stops publishing **without** saying
    /// so is still caught — this word only makes the console's sentence specific instead of
    /// merely alarmed. Believing the claim alone would trust the party least able to notice
    /// that it has stopped.
    Refusing = 4,
}

impl ProducerState {
    pub const fn from_wire(v: u32) -> Option<ProducerState> {
        match v {
            0 => Some(ProducerState::Starting),
            1 => Some(ProducerState::Producing),
            2 => Some(ProducerState::Paused),
            3 => Some(ProducerState::Gone),
            4 => Some(ProducerState::Refusing),
            _ => None,
        }
    }
    pub const fn to_wire(self) -> u32 {
        self as u32
    }
}

/// Why a producer cannot draw. A **closed set**, and small.
///
/// 🚨 **A name, never free text**, and the reason is not economy of bytes. A string a module
/// wrote, rendered inside the console's own chrome, is the module speaking in the console's
/// voice — the same objection that keeps a lighting scene a *name* rather than an RGB triple,
/// one layer over. The producer names a condition; the console owns the sentence.
///
/// ⚠️ **Two variants, because two are what exist.** Ascent's producer has exactly one real
/// refusal (a target it cannot draw into), and `region.rs`'s standing rule is that an
/// unreachable arm is an untested branch pretending to be a design. A third arrives with a
/// producer that has it, a wire tag, and a sentence — not before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum RefusalReason {
    /// The producer declined and did not say why. Also what a `Refusing` state means to a
    /// console too old to know a newer reason's tag.
    #[default]
    Unspecified = 0,
    /// The size the console is asking for is one this producer cannot draw at.
    SizeUnsupported = 1,
}

impl RefusalReason {
    pub const fn from_wire(v: u32) -> Option<RefusalReason> {
        match v {
            0 => Some(RefusalReason::Unspecified),
            1 => Some(RefusalReason::SizeUnsupported),
            _ => None,
        }
    }
    pub const fn to_wire(self) -> u32 {
        self as u32
    }

    /// The clause the console's sentence uses. Written here so the reason's words belong to
    /// the reason rather than being restated at every place a rectangle is drawn.
    pub const fn because(self) -> &'static str {
        match self {
            RefusalReason::Unspecified => "it is declining to draw",
            RefusalReason::SizeUnsupported => "it cannot draw at the size this viewport asks for",
        }
    }
}

// ---------------------------------------------------------------------------------------
// Geometry, and the header that records it
// ---------------------------------------------------------------------------------------

/// What the channel is sized for — **decided once, by the console, at creation.**
///
/// 🚨 This is the answer to *"which side owns the size?"*, first half. The console owns the
/// **capacity**: it is the party that knows how large a rectangle could ever become on this
/// display, and it is the party paying for the mapping. A producer cannot grow it, because a
/// producer able to grow a mapping the console allocated is a producer holding an allocation
/// verb it was never granted.
///
/// The second half is in [`crate::FrameView`]: the console **asks** for a size and the
/// producer **stamps what it actually drew** in every frame header. The request is advisory
/// with no deadline; the frame is the truth. That split is what makes a resize survive the
/// frames already in flight, which carry the old size and are still perfectly good pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCapacity {
    pub max_width: u32,
    pub max_height: u32,
    pub format: PixelFormat,
}

impl FrameCapacity {
    pub const fn new(max_width: u32, max_height: u32, format: PixelFormat) -> Self {
        FrameCapacity { max_width, max_height, format }
    }

    /// The padded row a frame of the full width occupies.
    pub const fn max_row_stride(&self) -> u32 {
        align_up((self.max_width * self.format.bytes_per_pixel()) as u64, ROW_ALIGNMENT as u64)
            as u32
    }

    /// One slot, header and pixels, aligned so every slot starts on a 256-byte boundary —
    /// which is what keeps each slot's atomics naturally aligned and each pixel block's rows
    /// aligned the way a GPU copy wants them.
    pub const fn slot_stride(&self) -> u64 {
        align_up(
            SLOT_HEADER_BYTES as u64 + self.max_row_stride() as u64 * self.max_height as u64,
            ROW_ALIGNMENT as u64,
        )
    }

    /// Is `(w, h)` a frame this channel can carry?
    pub const fn admits(&self, w: u32, h: u32) -> bool {
        w > 0 && h > 0 && w <= self.max_width && h <= self.max_height
    }
}

/// The channel's own description of itself, as written at byte 0 and validated on open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub wire_version: u32,
    pub slots: u32,
    pub capacity: FrameCapacity,
    pub slot_stride: u64,
    pub control_off: u64,
    pub status_off: u64,
    pub input_off: u64,
    pub slots_off: u64,
    pub input_capacity: u32,
}

impl Header {
    /// The layout a creator lays down for `capacity` — the one place the arithmetic lives.
    pub fn plan(capacity: FrameCapacity) -> Header {
        let control_off = HEADER_BYTES as u64;
        let status_off = control_off + control::BYTES as u64;
        let input_off = status_off + status::BYTES as u64;
        let input_bytes = input_ring::HEADER_BYTES as u64
            + input_ring::CAPACITY as u64 * input_ring::RECORD_BYTES as u64;
        let slots_off = align_up(input_off + input_bytes, ROW_ALIGNMENT as u64);
        Header {
            wire_version: WIRE_VERSION,
            slots: SLOTS,
            capacity,
            slot_stride: capacity.slot_stride(),
            control_off,
            status_off,
            input_off,
            slots_off,
            input_capacity: input_ring::CAPACITY,
        }
    }

    /// Total bytes the file must hold.
    pub fn total_bytes(&self) -> u64 {
        self.slots_off + self.slots as u64 * self.slot_stride
    }

    /// Where slot `i` starts.
    pub fn slot_off(&self, i: u32) -> u64 {
        self.slots_off + i as u64 * self.slot_stride
    }

    pub(crate) fn write(&self, dst: &mut [u8]) {
        dst[OFF_MAGIC..OFF_MAGIC + 8].copy_from_slice(&MAGIC);
        put_u32(dst, OFF_WIRE_VERSION, self.wire_version);
        put_u32(dst, OFF_HEADER_BYTES, HEADER_BYTES as u32);
        put_u32(dst, OFF_SLOTS, self.slots);
        put_u32(dst, OFF_FORMAT, self.capacity.format.to_wire());
        put_u32(dst, OFF_MAX_WIDTH, self.capacity.max_width);
        put_u32(dst, OFF_MAX_HEIGHT, self.capacity.max_height);
        put_u64(dst, OFF_SLOT_STRIDE, self.slot_stride);
        put_u64(dst, OFF_CONTROL_OFF, self.control_off);
        put_u64(dst, OFF_STATUS_OFF, self.status_off);
        put_u64(dst, OFF_INPUT_OFF, self.input_off);
        put_u64(dst, OFF_SLOTS_OFF, self.slots_off);
        put_u32(dst, OFF_INPUT_CAPACITY, self.input_capacity);
    }

    /// Read and **validate** a header out of a mapped file of `len` bytes.
    ///
    /// 🚨 The order of the checks is the design. Magic, then version, then everything else —
    /// because only the first two live at offsets a future build is promised, so a version
    /// mismatch must be diagnosed **before** any field whose position that version decides is
    /// read. Reading the geometry first and refusing on it would report a corrupt file where
    /// the truth is two builds that disagree.
    pub fn read(src: &[u8], len: u64) -> Result<Header, WireFault> {
        if src.len() < HEADER_BYTES {
            return Err(WireFault::TooSmall { bytes: src.len() as u64, need: HEADER_BYTES as u64 });
        }
        if src[OFF_MAGIC..OFF_MAGIC + 8] != MAGIC {
            return Err(WireFault::BadMagic);
        }
        let wire_version = get_u32(src, OFF_WIRE_VERSION);
        if wire_version != WIRE_VERSION {
            return Err(WireFault::VersionMismatch { ours: WIRE_VERSION, theirs: wire_version });
        }
        let format_tag = get_u32(src, OFF_FORMAT);
        let format =
            PixelFormat::from_wire(format_tag).ok_or(WireFault::UnknownFormat { tag: format_tag })?;
        let h = Header {
            wire_version,
            slots: get_u32(src, OFF_SLOTS),
            capacity: FrameCapacity {
                max_width: get_u32(src, OFF_MAX_WIDTH),
                max_height: get_u32(src, OFF_MAX_HEIGHT),
                format,
            },
            slot_stride: get_u64(src, OFF_SLOT_STRIDE),
            control_off: get_u64(src, OFF_CONTROL_OFF),
            status_off: get_u64(src, OFF_STATUS_OFF),
            input_off: get_u64(src, OFF_INPUT_OFF),
            slots_off: get_u64(src, OFF_SLOTS_OFF),
            input_capacity: get_u32(src, OFF_INPUT_CAPACITY),
        };
        // The file must be big enough for what it says it is. A header describing a channel
        // larger than the mapping is the shape every out-of-bounds read in this crate would
        // have to come through, so it is refused once, here, rather than bounds-checked at
        // every access site.
        if h.slots == 0 || h.slot_stride < SLOT_HEADER_BYTES as u64 {
            return Err(WireFault::Incoherent {
                why: "it describes zero slots, or a slot too small to hold a frame header",
            });
        }
        if h.capacity.max_width == 0 || h.capacity.max_height == 0 {
            return Err(WireFault::Incoherent {
                why: "it describes a capacity with no pixels in it",
            });
        }
        if h.slot_stride < h.capacity.slot_stride() {
            return Err(WireFault::Incoherent {
                why: "its slots are smaller than the capacity it claims",
            });
        }
        if h.control_off < HEADER_BYTES as u64
            || h.status_off < h.control_off + control::BYTES as u64
            || h.input_off < h.status_off + status::BYTES as u64
            || h.slots_off < h.input_off
        {
            return Err(WireFault::Incoherent { why: "its blocks overlap or run backwards" });
        }
        if h.total_bytes() > len {
            return Err(WireFault::TooSmall { bytes: len, need: h.total_bytes() });
        }
        Ok(h)
    }
}

/// Why a channel could not be believed. Every variant has a [`WireFault::sentence`], because
/// §4.6 wants a line in the rectangle and `module.rs` set the precedent that a refusal names
/// itself rather than sharing a "something went wrong".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireFault {
    TooSmall { bytes: u64, need: u64 },
    BadMagic,
    VersionMismatch { ours: u32, theirs: u32 },
    UnknownFormat { tag: u32 },
    Incoherent { why: &'static str },
}

impl WireFault {
    pub fn sentence(&self) -> String {
        match self {
            WireFault::TooSmall { bytes, need } => format!(
                "the frame channel is {bytes} bytes and the layout needs {need} — it was not \
                 finished being created, or it was truncated"
            ),
            WireFault::BadMagic => "the frame channel does not start with this protocol's magic \
                                    — it is some other file"
                .to_string(),
            WireFault::VersionMismatch { ours, theirs } => format!(
                "the module speaks frame-protocol version {theirs} and this Organon speaks \
                 {ours} — rebuild the module against this Organon, or this Organon against the \
                 module"
            ),
            WireFault::UnknownFormat { tag } => format!(
                "the frame channel names pixel format {tag}, which this build does not know"
            ),
            WireFault::Incoherent { why } => {
                format!("the frame channel's own header does not add up: {why}")
            }
        }
    }
}

impl fmt::Display for WireFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.sentence())
    }
}

impl std::error::Error for WireFault {}

// ---------------------------------------------------------------------------------------
// Little-endian primitives. Explicit, not `to_ne_bytes`.
// ---------------------------------------------------------------------------------------

/// ⚠️ **Little-endian on purpose, even though both parties are on one machine.**
/// `organon-core::ipc` uses native-endian and is right to: `Shared` is `#[repr(C)]` memory
/// shared between two builds of the SAME workspace. This channel is shared between two builds
/// of two DIFFERENT repositories, built independently, and "the same machine" is an
/// assumption about deployment rather than about the format. Pinning the order costs nothing
/// on every architecture this will ever run on, and removes a class of bug that would present
/// as a garbled picture.
#[inline]
pub(crate) fn put_u32(dst: &mut [u8], off: usize, v: u32) {
    dst[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

#[inline]
pub(crate) fn put_u64(dst: &mut [u8], off: usize, v: u64) {
    dst[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

#[inline]
pub(crate) fn get_u32(src: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(src[off..off + 4].try_into().unwrap())
}

#[inline]
pub(crate) fn get_u64(src: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(src[off..off + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 🚨 The two permanent commitments. If this test is ever "fixed" by moving a field, the
    /// fix is the bug: an older build reading a newer channel finds its magic and its version
    /// word exactly here, and that is the only reason it can refuse legibly instead of
    /// mis-reading the geometry.
    #[test]
    fn magic_and_version_sit_at_offsets_zero_and_eight() {
        assert_eq!(OFF_MAGIC, 0);
        assert_eq!(OFF_WIRE_VERSION, 8);
        let h = Header::plan(FrameCapacity::new(64, 64, PixelFormat::Rgba8UnormSrgb));
        let mut buf = vec![0u8; HEADER_BYTES];
        h.write(&mut buf);
        assert_eq!(&buf[0..8], b"ORGONMOD");
        assert_eq!(get_u32(&buf, 8), WIRE_VERSION);
    }

    #[test]
    fn a_version_mismatch_is_diagnosed_before_anything_positional() {
        let h = Header::plan(FrameCapacity::new(64, 64, PixelFormat::Rgba8UnormSrgb));
        let mut buf = vec![0u8; h.total_bytes() as usize];
        h.write(&mut buf);
        // A newer build wrote this, and reorganised everything below byte 12 while it was
        // there. The geometry is now nonsense to us — and we must still say "version", not
        // "incoherent".
        put_u32(&mut buf, OFF_WIRE_VERSION, WIRE_VERSION + 7);
        put_u32(&mut buf, OFF_SLOTS, 0);
        put_u64(&mut buf, OFF_SLOT_STRIDE, 1);
        match Header::read(&buf, buf.len() as u64) {
            Err(WireFault::VersionMismatch { ours, theirs }) => {
                assert_eq!(ours, WIRE_VERSION);
                assert_eq!(theirs, WIRE_VERSION + 7);
            }
            other => panic!("expected a version refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_header_that_does_not_fit_in_its_own_file_is_refused() {
        let h = Header::plan(FrameCapacity::new(256, 256, PixelFormat::Rgba8UnormSrgb));
        let mut buf = vec![0u8; h.total_bytes() as usize];
        h.write(&mut buf);
        let short = h.total_bytes() - 1;
        assert!(matches!(Header::read(&buf, short), Err(WireFault::TooSmall { .. })));
        assert!(Header::read(&buf, h.total_bytes()).is_ok());
    }

    #[test]
    fn a_header_claiming_slots_too_small_for_its_capacity_is_refused() {
        let cap = FrameCapacity::new(256, 256, PixelFormat::Rgba8UnormSrgb);
        let h = Header::plan(cap);
        let mut buf = vec![0u8; h.total_bytes() as usize * 2];
        h.write(&mut buf);
        put_u64(&mut buf, OFF_SLOT_STRIDE, cap.slot_stride() - ROW_ALIGNMENT as u64);
        assert!(matches!(
            Header::read(&buf, buf.len() as u64),
            Err(WireFault::Incoherent { .. })
        ));
    }

    #[test]
    fn overlapping_blocks_are_refused_rather_than_read_from() {
        let cap = FrameCapacity::new(256, 256, PixelFormat::Rgba8UnormSrgb);
        let h = Header::plan(cap);
        let mut buf = vec![0u8; h.total_bytes() as usize];
        h.write(&mut buf);
        // The status block now starts inside the control block, so the producer's writes
        // would land on the console's words.
        put_u64(&mut buf, OFF_STATUS_OFF, h.control_off + 8);
        assert!(matches!(
            Header::read(&buf, buf.len() as u64),
            Err(WireFault::Incoherent { .. })
        ));
    }

    #[test]
    fn the_header_round_trips() {
        let cap = FrameCapacity::new(1920, 1080, PixelFormat::Bgra8UnormSrgb);
        let h = Header::plan(cap);
        let mut buf = vec![0u8; h.total_bytes() as usize];
        h.write(&mut buf);
        assert_eq!(Header::read(&buf, buf.len() as u64).unwrap(), h);
    }

    /// The unaligned width T0's sweep carries deliberately, because a layout tested only on
    /// "nice" widths exercises the padding path zero times while looking complete.
    #[test]
    fn an_unaligned_width_pads_its_rows_and_the_slot_still_fits() {
        let cap = FrameCapacity::new(900, 506, PixelFormat::Rgba8UnormSrgb);
        assert_eq!(cap.max_row_stride(), 3840, "900 * 4 = 3600, padded to 3840");
        let h = Header::plan(cap);
        assert!(h.slot_stride >= SLOT_HEADER_BYTES as u64 + 3840 * 506);
        assert_eq!(h.slot_stride % ROW_ALIGNMENT as u64, 0);
        assert_eq!(h.slots_off % ROW_ALIGNMENT as u64, 0);
    }

    /// Every block a party writes must be inside the file, disjoint from the others, and
    /// aligned enough for the atomics that live in it.
    #[test]
    fn the_blocks_are_disjoint_ordered_and_aligned() {
        let h = Header::plan(FrameCapacity::new(640, 360, PixelFormat::Rgba8UnormSrgb));
        assert_eq!(h.control_off, HEADER_BYTES as u64);
        assert!(h.control_off + control::BYTES as u64 <= h.status_off);
        assert!(h.status_off + status::BYTES as u64 <= h.input_off);
        assert!(h.input_off < h.slots_off);
        for off in [h.control_off, h.status_off, h.input_off, h.slots_off] {
            assert_eq!(off % 8, 0, "an 8-byte atomic lives in every block");
        }
        for i in 0..h.slots {
            assert_eq!(h.slot_off(i) % 8, 0);
            assert!(h.slot_off(i) + h.slot_stride <= h.total_bytes());
        }
    }

    #[test]
    fn the_three_closed_vocabularies_round_trip_and_refuse_strangers() {
        for f in [PixelFormat::Rgba8UnormSrgb, PixelFormat::Bgra8UnormSrgb] {
            assert_eq!(PixelFormat::from_wire(f.to_wire()), Some(f));
        }
        assert_eq!(PixelFormat::from_wire(0), None);
        assert_eq!(PixelFormat::from_wire(99), None);
        for l in [Lifecycle::Attached, Lifecycle::Running] {
            assert_eq!(Lifecycle::from_wire(l.to_wire()), Some(l));
        }
        assert_eq!(Lifecycle::from_wire(2), None);
        for s in [
            ProducerState::Starting,
            ProducerState::Producing,
            ProducerState::Paused,
            ProducerState::Gone,
            ProducerState::Refusing,
        ] {
            assert_eq!(ProducerState::from_wire(s.to_wire()), Some(s));
        }
        assert_eq!(ProducerState::from_wire(5), None);
        for r in [RefusalReason::Unspecified, RefusalReason::SizeUnsupported] {
            assert_eq!(RefusalReason::from_wire(r.to_wire()), Some(r));
            assert!(!r.because().is_empty(), "a reason with no words is not a reason");
        }
        assert_eq!(RefusalReason::from_wire(2), None);
    }

    /// One seqlock implementation serves both blocks (`channel::BLOCK_SEQ`), which is only
    /// sound while both keep their counter at the same offset. Two `const`s in two modules
    /// agreeing by coincidence is exactly the kind of thing that stops being true in a
    /// refactor, silently, by bracketing the wrong four bytes.
    #[test]
    fn both_blocks_keep_their_counter_at_offset_zero() {
        assert_eq!(control::SEQ, 0);
        assert_eq!(status::SEQ, 0);
        assert_eq!(control::BYTES, status::BYTES, "and one read buffer serves both");
    }

    /// 🚨 **Every atomic lives OUTSIDE its block's seqlock body**, and this is the test that
    /// keeps it true. A seqlock brackets ordinary loads and stores; copying an atomic word as
    /// part of the body is a plain read racing an atomic write — the data race the lock exists
    /// to remove, moved rather than fixed. `organon-core::ipc` starts its body copy at byte 4
    /// for exactly this reason, and the same rule pushes `ALIVE`, `LATEST_SLOT`,
    /// `CONSOLE_ALIVE` and `READER_HOLD` past each `BODY_END`.
    #[test]
    fn no_atomic_sits_inside_a_seqlock_body() {
        for off in [control::SEQ, control::CONSOLE_ALIVE, control::READER_HOLD] {
            assert!(
                off == control::SEQ || off >= control::BODY_END,
                "control atomic at {off:#x} is inside the body it would race"
            );
        }
        for off in [status::SEQ, status::ALIVE, status::LATEST_SLOT] {
            assert!(
                off == status::SEQ || off >= status::BODY_END,
                "status atomic at {off:#x} is inside the body it would race"
            );
        }
        // And every body field is inside it, or a write would land nowhere at all. The `u64`s
        // are listed with their width, because a field that starts inside the body and ends
        // outside it is the same bug wearing a smaller hat.
        for (off, width) in [
            (control::LIFECYCLE, 4),
            (control::REQ_WIDTH, 4),
            (control::REQ_HEIGHT, 4),
            (control::SIZE_EPOCH, 8),
            (control::CLOSE, 4),
        ] {
            assert!((4..=control::BODY_END - width).contains(&off), "control {off:#x}");
        }
        for (off, width) in [
            (status::STATE, 4),
            (status::DRAWN_WIDTH, 4),
            (status::DRAWN_HEIGHT, 4),
            (status::FRAMES, 8),
            (status::APPLIED_SIZE_EPOCH, 8),
            (status::REFUSAL, 4),
        ] {
            assert!((4..=status::BODY_END - width).contains(&off), "status {off:#x}");
        }
        const { assert!(control::BODY_END <= control::BYTES) };
        const { assert!(status::BODY_END <= status::BYTES) };
    }

    /// `CLAUDE.md` invariant 4, pinned where it is decided rather than where it is described.
    #[test]
    fn a_module_arrives_inert() {
        assert_eq!(Lifecycle::default(), Lifecycle::Attached);
        assert_eq!(Lifecycle::default().to_wire(), 0, "and zeroed memory means it too");
    }
}
