//! **The preallocated frame ring — three slots, a seqlock each, and a reader's hold.**
//!
//! # 🚨 Preallocated is a condition of the measurement, not a preference
//!
//! `doc/measurements/module-frame-boundary-2026-08-21.md` measured mechanism B and found it
//! affordable — 0.44 ms round trip at 1280×720, 2.6 % of a 16.7 ms frame — **given a ring**.
//! The same document measured the naive shape beside it: a fresh staging buffer and a fresh
//! destination texture per iteration costs **2.72 ms at 1440p against 1.37 ms reused**, and
//! 3.3× of that gap is `memcpy out` copying *identical bytes* — first-touch page faulting,
//! not bandwidth.
//!
//! So a per-frame allocation measures 2.7 ms, reads as a verdict on the mechanism, and buys
//! `unsafe` per-backend GPU interop to fix an allocator problem. **The ring is the difference
//! between measuring the boundary and measuring `malloc`.** Every allocation in this file
//! happens at channel creation; nothing here allocates per frame, and
//! [`crate::ModuleChannel::staging_allocations`] exists so a test can prove it rather than a
//! comment claim it.
//!
//! # How a consumer knows a frame is complete rather than half-written
//!
//! Tearing is the failure the ring exists to prevent and it is invisible when it works, so
//! the mechanism is worth stating in full. Three things, and it takes all three:
//!
//! 1. **A per-slot seqlock.** The producer stamps the slot's counter **odd** before it
//!    touches a byte and **even** when the frame is whole. `organon-core::ipc`'s `Shared`
//!    runs on exactly this and its notes apply verbatim — in particular that the counter is
//!    a real `AtomicU32` rather than four bytes bracketed by fences, and that it must never
//!    *repeat* a value or a reader's before/after equality check passes over a body that
//!    changed underneath it. Here the counter only ever ascends: `begin` moves it to the next
//!    odd value from wherever it was, including from another odd value left by a producer
//!    that died mid-frame.
//! 2. **`latest_slot` is stamped only after that slot's seqlock closed.** A consumer never
//!    even looks at a slot the producer has not finished, so the seqlock's odd state is a
//!    *detector*, not the primary mechanism.
//! 3. **The reader publishes a hold before it reads, and the producer never writes a held
//!    slot.** With [`crate::wire::SLOTS`] = 3 there is always a slot that is neither the
//!    latest nor held, so this costs the producer nothing — it never waits.
//!
//! ⚠️ **(3) is a rule about a well-behaved producer and (1) is what survives one that is
//! not.** A buggy or hostile module can write anywhere in the mapping; the process boundary
//! bounds what it reaches through the protocol, not what it reaches on the machine. So the
//! consumer copies into its own staging buffer, **re-checks the counter afterwards**, and
//! throws the copy away if it moved. A torn read is a *skipped frame*, never a painted one.
//! `crate::channel::tests::a_producer_that_ignores_the_hold_is_caught_not_painted` drives
//! precisely that case.
//!
//! # What a producer that dies mid-frame leaves behind
//!
//! An odd counter and a slot full of half a picture — and `latest_slot` still naming the
//! previous, whole frame. So the death is **invisible to the frame path**: the consumer keeps
//! serving the last complete frame and never sees the wreckage. It becomes visible on the
//! *liveness* path instead ([`crate::Presence`]), which is the right place for it, because
//! "the producer stopped" is a fact about the producer rather than about a frame.

use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::map::Mapping;
use crate::wire::{
    get_u32, get_u64, put_u32, put_u64, slot, status, FrameCapacity, Header, PixelFormat,
    NO_SLOT, SLOT_HEADER_BYTES,
};

/// One complete frame, borrowed out of the consumer's own staging buffer.
///
/// 🚨 **The pixels are a copy, not a view of the mapping**, and that is the point: the copy is
/// what the seqlock re-check validates, so by the time this exists the bytes are provably one
/// frame rather than a blend of two. It is also the `memcpy out` T0 measured, so nothing is
/// spent here that the measurement did not already price.
#[derive(Clone, Copy)]
pub struct FrameView<'a> {
    /// 🚨 **The size the producer actually drew**, not the size the console asked for. This is
    /// the second half of "which side owns the size": the console asks, the producer answers
    /// per frame, and the frame is the truth. A frame still carrying the old size after a
    /// resize is a perfectly good picture that has not caught up yet, not an error.
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    /// Bytes between the starts of two rows in [`FrameView::pixels`]. Padded up to
    /// [`crate::wire::ROW_ALIGNMENT`], so it is usually larger than `width * bpp`.
    pub row_stride: u32,
    /// Counts from 1. Monotonic for the life of one producer; a restart begins again.
    pub frame_index: u64,
    /// The control block's size epoch this frame was drawn under — the answer to "has my
    /// resize landed?" that does not have to be inferred from dimensions that may coincide.
    pub size_epoch: u64,
    /// `row_stride * height` bytes.
    pub pixels: &'a [u8],
    published_unix_nanos: u64,
}

/// ⚠️ **Hand-written so a failing assertion stays readable.** A derived `Debug` prints
/// `pixels` — which is a whole frame, up to 14 MiB at 1440p — and every `assert!(matches!(…))`
/// in this crate's tests formats the `Poll` it did not expect. The derive turned one failed
/// expectation into a wall of bytes; this prints the header and the length.
impl std::fmt::Debug for FrameView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameView")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("row_stride", &self.row_stride)
            .field("frame_index", &self.frame_index)
            .field("size_epoch", &self.size_epoch)
            .field("pixels", &format_args!("{} bytes", self.pixels.len()))
            .finish()
    }
}

impl<'a> FrameView<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        width: u32,
        height: u32,
        format: PixelFormat,
        row_stride: u32,
        frame_index: u64,
        size_epoch: u64,
        published_unix_nanos: u64,
        pixels: &'a [u8],
    ) -> FrameView<'a> {
        FrameView {
            width,
            height,
            format,
            row_stride,
            frame_index,
            size_epoch,
            pixels,
            published_unix_nanos,
        }
    }
}

impl FrameView<'_> {
    /// How long ago the producer published this, or `None` if the answer cannot be trusted.
    ///
    /// 🚨 **This is the instrument for §4.4's number 2 — the one number the design refuses to
    /// let anybody estimate.** It is not that number: this is the age of *one frame at one
    /// poll*, and the question is how stale the *painted* frame is across two processes over
    /// a run. But the measurement is now a subtraction rather than a research project, which
    /// is what the design asked for when it said the number could not be taken until two
    /// processes existed.
    ///
    /// ⚠️ `None` on a negative or absurd result rather than a number nobody can stand behind.
    /// The two clocks are one machine's `SystemTime`, which is shared but **not monotonic** —
    /// an NTP step or a manual clock change mid-run makes the arithmetic meaningless, and a
    /// meaningless duration reported as a measurement is worse than a gap in the data.
    pub fn age(&self) -> Option<Duration> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_nanos();
        let then = self.published_unix_nanos as u128;
        if then == 0 || then > now {
            return None;
        }
        let d = Duration::from_nanos((now - then).min(u64::MAX as u128) as u64);
        // An hour-old frame is a clock event, not a stale picture: this protocol's consumer
        // declares a producer dead within seconds, so nothing can legitimately be that old.
        (d < Duration::from_secs(3600)).then_some(d)
    }

    /// The bytes of row `y`, without the padding.
    pub fn row(&self, y: u32) -> &[u8] {
        let bpp = self.format.bytes_per_pixel() as usize;
        let start = y as usize * self.row_stride as usize;
        &self.pixels[start..start + self.width as usize * bpp]
    }
}

/// A frame being written. Publishing is [`FrameWrite::commit`]; **dropping without it
/// publishes nothing**, which is exactly what a producer that dies mid-frame does.
///
/// 🚨 **There is deliberately no `Drop` impl.** A tidy-up on drop would be simulating a
/// courtesy a killed process cannot perform, and the consumer must survive the untidy case
/// anyway — so the untidy case is the only one, and it is the one the tests exercise. What is
/// left behind is an odd seqlock counter and a `latest_slot` still naming the previous whole
/// frame, which is why an abandoned write is invisible to the consumer rather than torn.
pub struct FrameWrite<'a> {
    map: &'a Mapping,
    slot_index: u32,
    slot_off: u64,
    /// Where `latest_slot` lives, carried rather than looked up so [`FrameWrite::commit`]
    /// needs no borrow of the header.
    status_latest_off: u64,
    /// The odd counter value this write is bracketed by.
    odd: u32,
    width: u32,
    height: u32,
    row_stride: u32,
    frame_index: u64,
}

impl FrameWrite<'_> {
    /// Bytes between the starts of two rows in [`FrameWrite::pixels_mut`].
    pub fn row_stride(&self) -> u32 {
        self.row_stride
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// This frame's index, stamped into the header. Counts from 1.
    pub fn frame_index(&self) -> u64 {
        self.frame_index
    }

    /// The whole padded pixel block, `row_stride * height` bytes.
    ///
    /// A producer that has just read back a GPU texture with `copy_texture_to_buffer` can
    /// `copy_from_slice` straight into this: [`crate::wire::ROW_ALIGNMENT`] is wgpu's own
    /// `COPY_BYTES_PER_ROW_ALIGNMENT`, so the two layouts agree and there is no repack.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = self.row_stride as usize * self.height as usize;
        // SAFETY: the producer is the sole writer of a slot body (map.rs rule 2), this slot
        // is neither the published one nor the one the consumer holds, and its seqlock is
        // open — so a consumer that reads it anyway discards what it read.
        unsafe { self.map.bytes_mut(self.slot_off + SLOT_HEADER_BYTES as u64, len) }
    }

    /// Row `y`'s padded bytes.
    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        let stride = self.row_stride as usize;
        let start = y as usize * stride;
        &mut self.pixels_mut()[start..start + stride]
    }

    /// Close the seqlock and publish this slot as the newest frame.
    ///
    /// The order is the whole of the mechanism and it cannot be rearranged: the publish
    /// timestamp, then the slot's own counter to even, then `latest_slot`. Stamping
    /// `latest_slot` first would advertise a slot whose counter is still odd — which a
    /// consumer would refuse, so it would present as *frames silently going missing* rather
    /// than as tearing, which is a harder thing to diagnose than the failure it causes.
    pub fn commit(self) {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        // SAFETY: sole writer of this slot's header, seqlock still open.
        let hdr = unsafe { self.map.bytes_mut(self.slot_off, SLOT_HEADER_BYTES) };
        put_u64(hdr, slot::PUBLISHED_UNIX_NANOS, now);
        self.map
            .u32_at(self.slot_off + slot::SEQ as u64)
            .store(self.odd.wrapping_add(1), Ordering::Release);
        // Only now is the slot advertisable. Release pairs with the consumer's acquire load.
        self.map.u32_at(self.status_latest_off).store(self.slot_index, Ordering::Release);
    }
}

impl<'a> FrameWrite<'a> {
    pub(crate) fn open(
        map: &'a Mapping,
        header: &Header,
        slot_index: u32,
        width: u32,
        height: u32,
        frame_index: u64,
        size_epoch: u64,
    ) -> FrameWrite<'a> {
        let slot_off = header.slot_off(slot_index);
        let seq = map.u32_at(slot_off + slot::SEQ as u64);
        // Next odd value from wherever the counter is, including from an odd one left by a
        // write that was abandoned. Never repeats, which is what the consumer's before/after
        // equality check requires.
        let current = seq.load(Ordering::Relaxed);
        let odd = if current & 1 == 1 { current.wrapping_add(2) } else { current.wrapping_add(1) };
        seq.store(odd, Ordering::Release);

        let bpp = header.capacity.format.bytes_per_pixel();
        let row_stride =
            crate::wire::align_up((width * bpp) as u64, crate::wire::ROW_ALIGNMENT as u64) as u32;

        // SAFETY: sole writer of this slot's header, seqlock now open.
        let hdr = unsafe { map.bytes_mut(slot_off, SLOT_HEADER_BYTES) };
        put_u32(hdr, slot::WIDTH, width);
        put_u32(hdr, slot::HEIGHT, height);
        put_u32(hdr, slot::FORMAT, header.capacity.format.to_wire());
        put_u32(hdr, slot::ROW_STRIDE, row_stride);
        put_u64(hdr, slot::FRAME_INDEX, frame_index);
        put_u64(hdr, slot::SIZE_EPOCH, size_epoch);
        put_u64(hdr, slot::PUBLISHED_UNIX_NANOS, 0);

        FrameWrite {
            map,
            slot_off,
            odd,
            width,
            height,
            row_stride,
            frame_index,
            slot_index,
            status_latest_off: header.status_off + status::LATEST_SLOT as u64,
        }
    }
}

/// Pick a slot that is neither the published one nor the one the consumer holds.
///
/// Returns `None` only if every slot is spoken for, which [`crate::wire::SLOTS`] = 3 makes
/// unreachable with one consumer — see that constant's docs. 🚨 **If it ever does happen the
/// answer is to publish nothing**, because the alternative is overwriting a slot somebody is
/// reading, and a dropped frame is recoverable in 16 ms while a torn one is a picture with
/// two halves of two worlds in it.
pub(crate) fn free_slot(map: &Mapping, header: &Header) -> Option<u32> {
    let latest = map.u32_at(header.status_off + status::LATEST_SLOT as u64).load(Ordering::Acquire);
    let held = map
        .u32_at(header.control_off + crate::wire::control::READER_HOLD as u64)
        .load(Ordering::Acquire);
    (0..header.slots).find(|&i| i != latest && i != held)
}

/// What one attempt to take the newest frame produced.
pub(crate) enum Taken {
    /// A whole frame is in the staging buffer; these are its header fields.
    Frame {
        width: u32,
        height: u32,
        format: PixelFormat,
        row_stride: u32,
        frame_index: u64,
        size_epoch: u64,
        published_unix_nanos: u64,
        bytes: usize,
    },
    /// Nothing has ever been published.
    Empty,
    /// The newest slot is one this consumer has already taken. 📌 Detected **before** the
    /// copy rather than after it: a console polling at 120 Hz against a 60 Hz producer sees
    /// this every other frame, and paying `memcpy out` for it would double the console's half
    /// of T0's number for no picture at all.
    Stale,
    /// The producer moved under us, or a slot was rewritten while we copied it. The caller
    /// retries; after the retries run out it keeps the picture it already had.
    Raced,
    /// A slot header that cannot be believed — a size outside the channel's own capacity, an
    /// unknown format, a row stride that does not fit. ⚠️ **This is not paranoia about a
    /// hypothetical.** The producer is another process's code; a field it computed wrongly
    /// would otherwise become a length in a `copy_from_slice` here.
    Corrupt,
}

/// One attempt at the newest complete frame, copied into `staging`.
///
/// `mid_copy` runs between the copy and the seqlock re-check. It is the no-op closure in
/// every real caller and exists so a test can write into the slot at exactly the moment the
/// re-check has to catch — see this module's docs on why (3) is a rule about a well-behaved
/// producer and (1) is what survives one that is not.
pub(crate) fn take_newest(
    map: &Mapping,
    header: &Header,
    staging: &mut [u8],
    newer_than: u64,
    mid_copy: &mut dyn FnMut(),
) -> Taken {
    let latest_off = header.status_off + status::LATEST_SLOT as u64;
    let hold_off = header.control_off + crate::wire::control::READER_HOLD as u64;

    let s = map.u32_at(latest_off).load(Ordering::Acquire);
    if s == NO_SLOT {
        return Taken::Empty;
    }
    if s >= header.slots {
        return Taken::Corrupt;
    }

    // Publish the hold BEFORE reading, so a producer choosing its next slot cannot choose
    // this one. SeqCst rather than Release: this store and the producer's load of the same
    // word are the two halves of a mutual-exclusion handshake, and the weaker orderings do
    // not give the store/load pair a single total order to agree on.
    map.u32_at(hold_off).store(s, Ordering::SeqCst);

    // Re-read. If the producer published a newer slot in the gap, our hold may have been
    // invisible when it chose — so `s` is a slot it might now be writing. Retry: the newer
    // slot is both fresher and provably not the one being raced, because our hold is now up.
    if map.u32_at(latest_off).load(Ordering::Acquire) != s {
        return Taken::Raced;
    }

    let slot_off = header.slot_off(s);
    let seq_cell = map.u32_at(slot_off + slot::SEQ as u64);
    let before = seq_cell.load(Ordering::Acquire);
    if before & 1 != 0 {
        // A write is in flight in the slot that is also the published one, which means the
        // producer wrote a held slot. Nothing under it is trustworthy.
        return Taken::Raced;
    }

    let hdr = map.bytes(slot_off, SLOT_HEADER_BYTES);
    let width = get_u32(hdr, slot::WIDTH);
    let height = get_u32(hdr, slot::HEIGHT);
    let row_stride = get_u32(hdr, slot::ROW_STRIDE);
    let Some(format) = PixelFormat::from_wire(get_u32(hdr, slot::FORMAT)) else {
        return Taken::Corrupt;
    };
    let frame_index = get_u64(hdr, slot::FRAME_INDEX);
    let size_epoch = get_u64(hdr, slot::SIZE_EPOCH);
    let published_unix_nanos = get_u64(hdr, slot::PUBLISHED_UNIX_NANOS);

    if !believable(&header.capacity, width, height, format, row_stride) {
        return Taken::Corrupt;
    }
    if frame_index <= newer_than {
        return Taken::Stale;
    }
    let bytes = row_stride as usize * height as usize;
    if bytes > staging.len() || slot_off + SLOT_HEADER_BYTES as u64 + bytes as u64 > map.len() as u64
    {
        return Taken::Corrupt;
    }

    staging[..bytes]
        .copy_from_slice(map.bytes(slot_off + SLOT_HEADER_BYTES as u64, bytes));

    mid_copy();

    if seq_cell.load(Ordering::Acquire) != before {
        // The slot was rewritten under us. The bytes in hand are a blend of two frames and
        // are thrown away rather than painted — §4.6's rule that a wrong picture is worse
        // than no picture, applied to the one case where the picture *looks* fine.
        return Taken::Raced;
    }

    Taken::Frame {
        width,
        height,
        format,
        row_stride,
        frame_index,
        size_epoch,
        published_unix_nanos,
        bytes,
    }
}

/// Is a slot header a thing this channel could actually have produced?
fn believable(
    cap: &FrameCapacity,
    width: u32,
    height: u32,
    format: PixelFormat,
    row_stride: u32,
) -> bool {
    if !cap.admits(width, height) || format != cap.format {
        return false;
    }
    let need = width * format.bytes_per_pixel();
    row_stride >= need && row_stride <= cap.max_row_stride()
}

/// Stop holding whatever slot was held.
pub(crate) fn release_hold(map: &Mapping, header: &Header) {
    map.u32_at(header.control_off + crate::wire::control::READER_HOLD as u64)
        .store(NO_SLOT, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::PixelFormat;

    #[test]
    fn a_believable_header_is_narrower_than_the_capacity_and_wider_than_the_pixels() {
        let cap = FrameCapacity::new(640, 360, PixelFormat::Rgba8UnormSrgb);
        assert!(believable(&cap, 640, 360, PixelFormat::Rgba8UnormSrgb, 2560));
        assert!(believable(&cap, 100, 10, PixelFormat::Rgba8UnormSrgb, 2560), "padding is fine");
        assert!(!believable(&cap, 641, 360, PixelFormat::Rgba8UnormSrgb, 2564), "too wide");
        assert!(!believable(&cap, 640, 0, PixelFormat::Rgba8UnormSrgb, 2560), "no pixels");
        assert!(
            !believable(&cap, 640, 360, PixelFormat::Rgba8UnormSrgb, 2559),
            "a stride that does not hold a row"
        );
        assert!(
            !believable(&cap, 640, 360, PixelFormat::Rgba8UnormSrgb, 4096),
            "a stride larger than the slot was sized for"
        );
        assert!(
            !believable(&cap, 640, 360, PixelFormat::Bgra8UnormSrgb, 2560),
            "a format the channel was not created with"
        );
    }
}
