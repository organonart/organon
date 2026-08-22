//! **The two halves — [`ModuleChannel`] (the console) and [`ProducerChannel`] (the module).**
//!
//! # 🚨 One writer per block, and the API is what enforces it
//!
//! `map.rs`'s rule 2 says every non-atomic byte range has exactly one writer. This file is
//! where that stops being a convention: **[`ProducerChannel`] has no method that writes a
//! control word, and [`ModuleChannel`] has no method that writes a status word.** There is no
//! discipline to remember, because there is nothing to call.
//!
//! That is also what makes §5.1's default structural. `Lifecycle::Attached` is written by
//! [`ModuleChannel::create`] and can only be changed by
//! [`ModuleChannel::set_lifecycle`] — a producer cannot start itself, so a manifest cannot
//! ask to be started, so §3.1's rule survives contact with a running channel.
//!
//! ⚠️ **The one exception, at the one moment it is safe.** [`ModuleChannel::create`] writes
//! two producer-owned words — `latest_slot` and the initial `ProducerState` — because a
//! freshly zeroed file would otherwise say *"slot 0 holds the newest frame"*, which is a lie
//! that reads as a black picture rather than as an empty channel. It is written before the
//! producer process exists, so there is no second party to race.
//!
//! # Which side owns the size, in three sentences
//!
//! 1. The console owns the **capacity**, once, at creation ([`crate::FrameCapacity`]) — it
//!    is the party paying for the mapping and the only one that knows how big the rectangle
//!    could ever get.
//! 2. The console **asks** for a size ([`ModuleChannel::ask_size`]), with no deadline.
//! 3. The producer **answers per frame** ([`crate::FrameView::width`]), and the frame is the
//!    truth.
//!
//! 📌 The consequence worth stating, because it is what makes a resize cheap: frames already
//! in flight when the request changed carry the old size and are **perfectly good pictures**.
//! Nothing has to be flushed, nothing is invalid, and the console scales one frame oddly for
//! a few milliseconds instead of showing a gap. `size_epoch` is how it knows which is which
//! without inferring it from dimensions that may coincide — a 640×360 request answered by a
//! 640×360 frame is ambiguous between "it caught up" and "it has not moved yet", and the
//! epoch is not.
//!
//! ⚠️ **A producer may reallocate on a size change**, and Ascent does — its depth texture goes
//! with it. So the request is deliberately *level-triggered*, not an event: it is a value the
//! producer reads when it is ready, so an oscillating console does not enqueue a queue of
//! reallocations, it just moves a number the producer will read once.

use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::input::{DrainReport, InputEvent, PushFault};
use crate::map::Mapping;
use crate::presence::{Poll, Presence, Timings};
use crate::ring::{self, FrameView, Taken};
use crate::wire::{
    control, get_u32, get_u64, put_u32, put_u64, status, FrameCapacity, Header, Lifecycle,
    ProducerState, WireFault, NO_SLOT,
};

/// The file name a channel for `producer` uses, without a directory.
///
/// 🚨 **This crate names the file and the console places it.** The right directory is
/// `organon-core::ipc::ns_file`'s — every Organon mmap and sidecar goes through it, and
/// `$ORGANON_IPC_NS` is what lets a Console session and an Organon session coexist. But
/// resolving it is one line at the console's call site, and taking a dependency on the
/// engine's spine to get a `PathBuf` would put `glam`, `half`, `bytemuck`, `serde` and
/// `serde_json` into a game's build for a string. So: a pure function here, a prefix there.
///
/// `instance` distinguishes two viewports holding the same producer. §4.2 moves
/// `only_one_because` from the content word to the producer precisely so a hosted module can
/// answer "as many as you like" — a separate process rendering into its own texture has no
/// jitter phase to trade — and two channels must then not be one file.
pub fn channel_file_name(producer: &str, instance: u32) -> String {
    format!("module-{producer}-{instance}.frames")
}

/// Why a producer could not attach to a channel.
#[derive(Debug)]
pub enum OpenFault {
    /// The file is not there, or could not be mapped. Usually: the console has not created it
    /// yet, or the producer was handed the wrong path.
    Io(io::Error),
    /// The bytes are there and cannot be believed. [`WireFault::sentence`] says why.
    Wire(WireFault),
}

impl OpenFault {
    pub fn sentence(&self) -> String {
        match self {
            OpenFault::Io(e) => format!("the frame channel could not be opened: {e}"),
            OpenFault::Wire(w) => w.sentence(),
        }
    }
}

impl std::fmt::Display for OpenFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence())
    }
}

impl std::error::Error for OpenFault {}

// ---------------------------------------------------------------------------------------
// Seqlock over a small block. Both directions, one implementation.
// ---------------------------------------------------------------------------------------

/// The largest block a seqlock read copies. Both `control` and `status` are 64.
const BLOCK_MAX: usize = 64;

/// How many times a block read retries before giving up and telling the caller to keep what
/// it had. A 64-byte block written a few times a second cannot lose eight races unless the
/// writer is stuck mid-write — in which case "keep the previous snapshot" is exactly right,
/// and the liveness counter is the thing that will notice.
const BLOCK_RETRIES: usize = 8;

/// ⚠️ **Both blocks put their seqlock counter at offset 0, and this depends on it.** That is
/// an agreement between two `const`s in two modules, so it is asserted rather than assumed —
/// `wire::tests::both_blocks_keep_their_counter_at_offset_zero` fails if either moves, which
/// is the only way this function could otherwise start bracketing the wrong four bytes.
const BLOCK_SEQ: u64 = control::SEQ as u64;

fn block_write(map: &Mapping, off: u64, bytes: usize, body: impl FnOnce(&mut [u8])) {
    let seq = map.u32_at(off + BLOCK_SEQ);
    let cur = seq.load(Ordering::Relaxed);
    let odd = if cur & 1 == 1 { cur.wrapping_add(2) } else { cur.wrapping_add(1) };
    seq.store(odd, Ordering::Release);
    // SAFETY: the caller owns this block (see the module docs), and the seqlock is open, so a
    // reader that catches the write discards what it read.
    let raw = unsafe { map.bytes_mut(off, bytes) };
    body(raw);
    seq.store(odd.wrapping_add(1), Ordering::Release);
}

/// `None` when every attempt caught a write in flight.
fn block_read(map: &Mapping, off: u64, bytes: usize) -> Option<[u8; BLOCK_MAX]> {
    let seq = map.u32_at(off + BLOCK_SEQ);
    for _ in 0..BLOCK_RETRIES {
        let before = seq.load(Ordering::Acquire);
        if before & 1 != 0 {
            std::hint::spin_loop();
            continue;
        }
        let mut buf = [0u8; BLOCK_MAX];
        buf[..bytes].copy_from_slice(map.bytes(off, bytes));
        if seq.load(Ordering::Acquire) == before {
            return Some(buf);
        }
        std::hint::spin_loop();
    }
    None
}

// ---------------------------------------------------------------------------------------
// The console's half
// ---------------------------------------------------------------------------------------

/// What the producer last said about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProducerStatus {
    pub state: ProducerState,
    /// The size the producer says it is drawing at. ⚠️ **Advisory.** The per-frame size is the
    /// truth; this is for a status line, never for sizing a texture.
    pub drawn: (u32, u32),
    pub frames: u64,
    /// The `size_epoch` the producer has adopted. Equal to
    /// [`ModuleChannel::size_epoch`] once a resize has landed.
    pub applied_size_epoch: u64,
}

impl Default for ProducerStatus {
    fn default() -> Self {
        ProducerStatus {
            state: ProducerState::Starting,
            drawn: (0, 0),
            frames: 0,
            applied_size_epoch: 0,
        }
    }
}

/// **The console's end of one hosted module's viewport.**
///
/// Everything preallocated at [`ModuleChannel::create`]: the mapping, its three slots, and
/// the one staging buffer every frame is copied into. Nothing in [`ModuleChannel::poll`]
/// allocates — see [`ModuleChannel::staging_allocations`], which exists so a test can say so.
pub struct ModuleChannel {
    map: Mapping,
    header: Header,
    /// Capacity-sized once. **Not** resized when the requested size changes, which is the
    /// whole point: a size change must not become an allocation on the frame path.
    staging: Vec<u8>,
    staging_allocations: u64,
    timings: Timings,
    opened_at: Instant,
    size_epoch: u64,
    requested: (u32, u32),
    lifecycle: Lifecycle,
    status: ProducerStatus,
    last_alive: u64,
    last_alive_at: Instant,
    last_frame_index: u64,
    torn_reads: u64,
    corrupt_reads: u64,
}

impl ModuleChannel {
    /// Create the channel file and lay the protocol down in it.
    ///
    /// The producer is expected not to exist yet: this is the console preparing the ground
    /// before it spawns anything.
    pub fn create(path: &Path, capacity: FrameCapacity) -> io::Result<ModuleChannel> {
        let header = Header::plan(capacity);
        let map = Mapping::create(path, header.total_bytes())?;
        {
            // SAFETY: nothing else has this file open — the producer process does not exist
            // yet, which is the whole reason `create` may write producer-owned words below.
            let head = unsafe { map.bytes_mut(0, crate::wire::HEADER_BYTES) };
            header.write(head);
        }
        // The two producer-owned words the console must seed — see the module docs.
        map.u32_at(header.status_off + status::LATEST_SLOT as u64)
            .store(NO_SLOT, Ordering::Release);
        map.u32_at(header.status_off + status::STATE as u64)
            .store(ProducerState::Starting.to_wire(), Ordering::Release);
        map.u32_at(header.control_off + control::READER_HOLD as u64)
            .store(NO_SLOT, Ordering::Release);

        let now = Instant::now();
        let staging = vec![0u8; (capacity.slot_stride() - crate::wire::SLOT_HEADER_BYTES as u64) as usize];
        let mut ch = ModuleChannel {
            map,
            header,
            staging,
            staging_allocations: 1,
            timings: Timings::default(),
            opened_at: now,
            size_epoch: 0,
            requested: (capacity.max_width, capacity.max_height),
            lifecycle: Lifecycle::Attached,
            status: ProducerStatus::default(),
            last_alive: 0,
            last_alive_at: now,
            last_frame_index: 0,
            torn_reads: 0,
            corrupt_reads: 0,
        };
        ch.write_control();
        ch.heartbeat();
        Ok(ch)
    }

    /// Override the liveness policy. Tests do; the console takes the default.
    pub fn with_timings(mut self, timings: Timings) -> Self {
        self.timings = timings;
        self
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn capacity(&self) -> FrameCapacity {
        self.header.capacity
    }

    /// How many times the staging buffer has been allocated. **Always 1.**
    ///
    /// 🚨 This is a measurement, not a statistic: T0's finding is that a per-frame allocation
    /// costs double and reads as a verdict on the mechanism, so the absence of one is a
    /// property worth being able to assert rather than a comment worth believing.
    pub fn staging_allocations(&self) -> u64 {
        self.staging_allocations
    }

    /// How many frames were thrown away because a slot changed under the copy.
    ///
    /// Zero against a producer that honours the reader's hold. Non-zero means one does not —
    /// which is a producer bug, and one worth being able to see rather than one that presents
    /// as an occasional strange frame. There is deliberately no `Poll::Torn`; see [`Poll`].
    pub fn torn_reads(&self) -> u64 {
        self.torn_reads
    }

    /// How many times a slot header could not be believed — a size outside the channel's own
    /// capacity, an unknown format, a stride that does not fit. Non-zero means the producer
    /// is writing outside the protocol.
    pub fn corrupt_reads(&self) -> u64 {
        self.corrupt_reads
    }

    /// The epoch the console is currently asking for. Compare with
    /// [`ProducerStatus::applied_size_epoch`] to know whether a resize has landed.
    pub fn size_epoch(&self) -> u64 {
        self.size_epoch
    }

    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// The producer's own last claim about itself. ⚠️ A claim — [`Poll::presence`] is what a
    /// rectangle should be drawn from.
    pub fn status(&self) -> ProducerStatus {
        self.status
    }

    /// Ask the producer to draw at `w` × `h`.
    ///
    /// Clamped to the channel's capacity rather than refused: a console asking for more than
    /// the mapping holds is a console whose region grew past what it planned for, and the
    /// right answer is a slightly small picture now plus a channel rebuilt at leisure, not a
    /// black rectangle. Returns whether the request actually changed.
    pub fn ask_size(&mut self, w: u32, h: u32) -> bool {
        let w = w.clamp(1, self.header.capacity.max_width);
        let h = h.clamp(1, self.header.capacity.max_height);
        if (w, h) == self.requested {
            return false;
        }
        self.requested = (w, h);
        self.size_epoch += 1;
        self.write_control();
        true
    }

    /// §5.1's two states. The console is the only party that can move this.
    pub fn set_lifecycle(&mut self, lifecycle: Lifecycle) {
        if self.lifecycle != lifecycle {
            self.lifecycle = lifecycle;
            self.write_control();
        }
    }

    /// Ask the producer to exit. It is a request, not a kill — the kill is the launcher's, and
    /// the launcher is T3b's.
    pub fn request_close(&mut self) {
        self.write_control_with(|raw| put_u32(raw, control::CLOSE, 1));
    }

    /// Bump the console's own liveness counter. Call once per console frame.
    ///
    /// 📌 Symmetry, and it is not decoration: a producer whose console has gone away is an
    /// orphan holding a GPU, and this is the only way it can find out without polling for a
    /// parent process it may not be able to see.
    pub fn heartbeat(&mut self) {
        self.map
            .u64_at(self.header.control_off + control::CONSOLE_ALIVE as u64)
            .fetch_add(1, Ordering::Release);
    }

    /// Send one input event.
    ///
    /// [`PushFault::Refused`] means the protocol does not carry it — see [`crate::input`]'s
    /// table of refusals, and §5.3's reserved key.
    pub fn send(&mut self, ev: InputEvent) -> Result<(), PushFault> {
        crate::input::push(&self.map, &self.header, ev)
    }

    /// Take the newest complete frame, or find out why there is not one.
    ///
    /// 🚨 **`Poll::Frame` is unreachable once the producer is judged stalled, lost or gone**,
    /// and the ring is not even read in those states. That is §4.6's *never the last good
    /// frame*, made a property of the code path rather than a rule at the call site.
    pub fn poll(&mut self, now: Instant) -> Poll<'_> {
        self.poll_interfered(now, &mut || {})
    }

    /// [`ModuleChannel::poll`], with a hook that runs between the pixel copy and the seqlock
    /// re-check.
    ///
    /// The hook is a no-op in every real caller. It exists so a test can be a **hostile
    /// producer** — writing into the slot at exactly the moment the tear detector has to
    /// catch it — without threads and without a timing race, which is the only way to prove a
    /// detector that is silent whenever it works. See
    /// `tests::a_producer_that_ignores_the_hold_is_caught_not_painted`.
    pub fn poll_interfered(&mut self, now: Instant, mid_copy: &mut dyn FnMut()) -> Poll<'_> {
        self.refresh_status(now);

        let silent = now.saturating_duration_since(self.last_alive_at);
        let since_open = now.saturating_duration_since(self.opened_at);
        let presence = Presence::judge(
            self.status.state,
            self.status.frames > 0,
            silent,
            since_open,
            &self.timings,
        );
        match presence {
            Presence::Starting { elapsed } => return Poll::Starting { elapsed },
            Presence::Stalled { silent } => return Poll::Stalled { silent },
            Presence::Lost { silent } => return Poll::Lost { silent },
            Presence::Gone => return Poll::Gone,
            Presence::Live => {}
        }

        // Bounded: a producer publishing faster than this consumer can start a copy would
        // otherwise spin here for ever. Running out is not an error — it is "no new frame
        // this poll", and the next poll is 16 ms away.
        const TAKE_RETRIES: usize = 8;
        let mut torn = 0u64;
        let mut corrupt = 0u64;
        let mut got = None;
        for _ in 0..TAKE_RETRIES {
            match ring::take_newest(
                &self.map,
                &self.header,
                &mut self.staging,
                self.last_frame_index,
                mid_copy,
            ) {
                Taken::Frame {
                    width,
                    height,
                    format,
                    row_stride,
                    frame_index,
                    size_epoch,
                    published_unix_nanos,
                    bytes,
                } => {
                    got = Some((
                        width,
                        height,
                        format,
                        row_stride,
                        frame_index,
                        size_epoch,
                        published_unix_nanos,
                        bytes,
                    ));
                    break;
                }
                Taken::Empty | Taken::Stale => break,
                Taken::Raced => torn += 1,
                Taken::Corrupt => {
                    corrupt += 1;
                    break;
                }
            }
        }
        ring::release_hold(&self.map, &self.header);
        // 🚨 A retry that later succeeded was still a real race and is still counted: the
        // number this exposes is "how often does the producer write a slot somebody is
        // holding", and swallowing the recovered ones would make a misbehaving producer look
        // clean until it happened to lose the last retry too.
        self.torn_reads += torn;
        self.corrupt_reads += corrupt;

        let Some((w, h, format, row_stride, frame_index, size_epoch, published, bytes)) = got
        else {
            return Poll::Holding;
        };
        self.last_frame_index = frame_index;
        Poll::Frame(FrameView::new(
            w,
            h,
            format,
            row_stride,
            frame_index,
            size_epoch,
            published,
            &self.staging[..bytes],
        ))
    }

    fn refresh_status(&mut self, now: Instant) {
        let alive =
            self.map.u64_at(self.header.status_off + status::ALIVE as u64).load(Ordering::Acquire);
        if alive != self.last_alive {
            self.last_alive = alive;
            self.last_alive_at = now;
        }
        if let Some(raw) = block_read(&self.map, self.header.status_off, status::BYTES) {
            if let Some(state) = ProducerState::from_wire(get_u32(&raw, status::STATE)) {
                self.status = ProducerStatus {
                    state,
                    drawn: (get_u32(&raw, status::DRAWN_WIDTH), get_u32(&raw, status::DRAWN_HEIGHT)),
                    frames: get_u64(&raw, status::FRAMES),
                    applied_size_epoch: get_u64(&raw, status::APPLIED_SIZE_EPOCH),
                };
            }
            // An unknown state word keeps the previous snapshot: a newer producer's state is
            // not a reason to forget everything else it said.
        }
    }

    fn write_control(&mut self) {
        let (w, h) = self.requested;
        let epoch = self.size_epoch;
        let life = self.lifecycle.to_wire();
        self.write_control_with(move |raw| {
            put_u32(raw, control::LIFECYCLE, life);
            put_u32(raw, control::REQ_WIDTH, w);
            put_u32(raw, control::REQ_HEIGHT, h);
            put_u64(raw, control::SIZE_EPOCH, epoch);
        });
    }

    fn write_control_with(&mut self, body: impl FnOnce(&mut [u8])) {
        block_write(&self.map, self.header.control_off, control::BYTES, body);
    }
}

// ---------------------------------------------------------------------------------------
// The producer's half
// ---------------------------------------------------------------------------------------

/// What the console is currently asking for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub lifecycle: Lifecycle,
    /// The size to draw at. Level-triggered — read it when convenient, not on an event.
    pub requested: (u32, u32),
    pub size_epoch: u64,
    /// The console has asked this producer to exit.
    pub close: bool,
}

/// **The module's end.** Opens a channel the console created; writes the status block, drains
/// the input ring, publishes frames.
///
/// It has no method that writes a control word. See the module docs.
pub struct ProducerChannel {
    map: Mapping,
    header: Header,
    frames: u64,
    state: ProducerState,
    control: Control,
    applied_size_epoch: u64,
    drawn: (u32, u32),
    /// Reused across drains — the input path allocates once too.
    inbox: Vec<InputEvent>,
}

impl ProducerChannel {
    /// Attach to a channel the console created.
    ///
    /// ⚠️ **Exactly one producer per channel, and this is not enforced.** Opening resets the
    /// status block — `Starting`, zero frames — so a second `open` against a live channel
    /// makes the console believe its producer just restarted, and both processes would then
    /// write the same slots. The console's side of the rule is
    /// [`channel_file_name`]'s `instance`: two viewports holding the same producer get two
    /// files, so the only way to reach this state is to launch two producers against one path.
    /// The launcher that could do that is T3b's, and this is the note it is owed.
    pub fn open(path: &Path) -> Result<ProducerChannel, OpenFault> {
        let map = Mapping::open(path).map_err(OpenFault::Io)?;
        let len = map.len() as u64;
        let header =
            Header::read(map.head(crate::wire::HEADER_BYTES), len).map_err(OpenFault::Wire)?;
        let mut ch = ProducerChannel {
            map,
            header,
            frames: 0,
            state: ProducerState::Starting,
            control: Control {
                lifecycle: Lifecycle::Attached,
                requested: (header.capacity.max_width, header.capacity.max_height),
                size_epoch: 0,
                close: false,
            },
            applied_size_epoch: 0,
            drawn: (0, 0),
            inbox: Vec::with_capacity(crate::wire::input_ring::CAPACITY as usize + 1),
        };
        ch.read_control();
        ch.write_status();
        Ok(ch)
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn capacity(&self) -> FrameCapacity {
        self.header.capacity
    }

    /// The console's current wishes, as of the last [`ProducerChannel::tick`] or
    /// [`ProducerChannel::open`].
    pub fn control(&self) -> Control {
        self.control
    }

    /// Has the console gone away?
    ///
    /// Answers by the same means the console judges the producer: a counter that moved. The
    /// caller keeps the previous value; two identical readings a few seconds apart mean
    /// nobody is watching.
    pub fn console_alive(&self) -> u64 {
        self.map
            .u64_at(self.header.control_off + control::CONSOLE_ALIVE as u64)
            .load(Ordering::Acquire)
    }

    /// **Call once per producer loop iteration, whether or not a frame is drawn.**
    ///
    /// 🚨 This is the whole of "paused versus hung". A producer that only bumped its counter
    /// when it published a frame would be indistinguishable from a wedged one the moment the
    /// console paused it — and pausing is the *default* state a module arrives in, so that
    /// mistake would make the normal case look like the failure case.
    ///
    /// It also re-reads the control block, so a producer that ticks cannot fail to notice a
    /// resize, a lifecycle change or a close request.
    pub fn tick(&mut self) {
        self.map
            .u64_at(self.header.status_off + status::ALIVE as u64)
            .fetch_add(1, Ordering::Release);
        self.read_control();
    }

    /// Everything the console has sent since the last call.
    ///
    /// The [`DrainReport`] is worth reading: `recovered_from_overflow` means the buffered
    /// events were **discarded** and one [`InputEvent::ReleaseAll`] delivered instead, which
    /// is [`crate::input::drain`]'s deliberate trade.
    pub fn drain_input(&mut self) -> (&[InputEvent], DrainReport) {
        self.inbox.clear();
        let report = crate::input::drain(&self.map, &self.header, &mut self.inbox);
        (&self.inbox, report)
    }

    /// Say what this producer is doing. `Gone` is a farewell — see
    /// [`ProducerChannel::depart`].
    pub fn set_state(&mut self, state: ProducerState) {
        if self.state != state {
            self.state = state;
            self.write_status();
        }
    }

    /// Record that this producer has adopted the console's current requested size.
    ///
    /// 📌 Separate from drawing a frame at that size on purpose: a producer that reallocates
    /// a depth buffer before its first frame at the new size has *adopted* the request
    /// already, and a console that could only learn this from a frame would report the resize
    /// as not-landed through the reallocation.
    pub fn adopt_size(&mut self, w: u32, h: u32) {
        self.drawn = (w, h);
        self.applied_size_epoch = self.control.size_epoch;
        self.write_status();
    }

    /// Begin a frame. `None` if the size is outside the channel's capacity, or — see
    /// [`crate::ring::free_slot`] — if every slot is spoken for, which three slots and one
    /// consumer make unreachable.
    ///
    /// Publishing is [`crate::FrameWrite::commit`]. **Dropping without it publishes nothing**,
    /// which is exactly what a producer that dies mid-frame does, and the consumer never sees
    /// the wreckage.
    pub fn begin_frame(&mut self, w: u32, h: u32) -> Option<crate::FrameWrite<'_>> {
        if !self.header.capacity.admits(w, h) {
            return None;
        }
        let slot = ring::free_slot(&self.map, &self.header)?;
        self.frames += 1;
        let frame_index = self.frames;
        let epoch = self.control.size_epoch;
        Some(crate::FrameWrite::open(&self.map, &self.header, slot, w, h, frame_index, epoch))
    }

    /// Record a published frame in the status block. Call after
    /// [`crate::FrameWrite::commit`].
    ///
    /// ⚠️ **Separate from `commit` because the two blocks have different writers' disciplines
    /// and different costs.** `commit` closes a seqlock and stores one word — it is on the
    /// frame path and must stay that size. This takes the status block's seqlock, which is
    /// the same lock the console reads for liveness, and folding it into `commit` would put a
    /// second lock acquisition inside the frame publish for a number nothing reads per frame.
    pub fn published(&mut self, w: u32, h: u32) {
        self.drawn = (w, h);
        if self.state == ProducerState::Starting {
            self.state = ProducerState::Producing;
        }
        self.write_status();
    }

    /// Say goodbye. §4.6's difference between *exited* and *hung*, and the cheapest thing in
    /// the protocol.
    pub fn depart(&mut self) {
        self.set_state(ProducerState::Gone);
    }

    fn read_control(&mut self) {
        let Some(raw) = block_read(&self.map, self.header.control_off, control::BYTES) else {
            return; // keep the previous snapshot rather than act on a torn one
        };
        let Some(lifecycle) = Lifecycle::from_wire(get_u32(&raw, control::LIFECYCLE)) else {
            // A newer console's lifecycle value. ⚠️ Keeping the previous one is the safe
            // direction *because the default is inert*: the worst outcome is a module that
            // stays paused, never one that starts itself because it did not understand.
            return;
        };
        self.control = Control {
            lifecycle,
            requested: (get_u32(&raw, control::REQ_WIDTH), get_u32(&raw, control::REQ_HEIGHT)),
            size_epoch: get_u64(&raw, control::SIZE_EPOCH),
            close: get_u32(&raw, control::CLOSE) != 0,
        };
    }

    fn write_status(&mut self) {
        let (state, frames, drawn, epoch) =
            (self.state.to_wire(), self.frames, self.drawn, self.applied_size_epoch);
        block_write(&self.map, self.header.status_off, status::BYTES, move |raw| {
            put_u32(raw, status::STATE, state);
            put_u32(raw, status::DRAWN_WIDTH, drawn.0);
            put_u32(raw, status::DRAWN_HEIGHT, drawn.1);
            put_u64(raw, status::FRAMES, frames);
            put_u64(raw, status::APPLIED_SIZE_EPOCH, epoch);
        });
    }
}

impl ProducerChannel {
    /// How many frames this producer has begun. Counts from 1 for the first.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// What this producer last said about itself. ⚠️ Its own claim, and it deliberately carries
    /// no clock: the console owns the judgement about liveness, because the producer is
    /// exactly the party that cannot notice it has stopped.
    pub fn state(&self) -> ProducerState {
        self.state
    }
}

/// Not public API: the tear-detector test needs to write a slot while ignoring the hold, which
/// is the one thing a well-behaved producer never does.
#[cfg(any(test, feature = "sim"))]
impl ProducerChannel {
    /// 🚨 **Write a slot the consumer may be reading.** A misbehaving producer, on purpose.
    ///
    /// Available only under `cfg(test)` or the `sim` feature, because a shipped build has no
    /// business being able to do this. It is here rather than in a test file because it needs
    /// the mapping, and a test that reached the mapping through a public accessor would have
    /// required adding one — which would put the capability in every build to test that it is
    /// caught.
    pub fn scribble_over(&mut self, slot_index: u32, byte: u8) {
        let off = self.header.slot_off(slot_index);
        let seq = self.map.u32_at(off + crate::wire::slot::SEQ as u64);
        let cur = seq.load(Ordering::Relaxed);
        seq.store(cur.wrapping_add(1), Ordering::Release);
        let len = (self.header.slot_stride - crate::wire::SLOT_HEADER_BYTES as u64) as usize;
        // SAFETY: the test is the only writer; the point is precisely that the consumer's
        // detector must survive this.
        unsafe { self.map.bytes_mut(off + crate::wire::SLOT_HEADER_BYTES as u64, len) }.fill(byte);
        seq.store(cur.wrapping_add(2), Ordering::Release);
    }
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
