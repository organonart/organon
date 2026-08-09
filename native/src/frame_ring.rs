//! **The frame mirror ring** (#554 Tier 1) — the visual's rendered frames, carried to the
//! editor so it can draw a live viewport inside its own window.
//!
//! # Why a ring and not a shared GPU texture
//!
//! The obvious design is zero-copy: render into a texture and let egui sample it. That is
//! [#541 Tier 4](https://github.com/organonart/organon/issues/541). It is blocked *by cargo* —
//! no published `egui-wgpu` accepts wgpu 30 (`egui-wgpu 0.33` → wgpu `^27`; 0.34/0.35 → `^29`),
//! and a wgpu-30 texture cannot be handed to wgpu-27 because they are different crates as far
//! as Rust is concerned.
//!
//! So the boundary is CPU memory instead. egui does not care what produced the pixels, and a
//! `memcpy` is version-agnostic in a way a GPU handle never will be.
//!
//! ## ⚠️ Correction (#554 Tier 4): the *constraint* was real, the *conclusion* was not
//!
//! This module originally went on to say that closing the gap "would mean forking
//! `egui-baseview`, porting it, and **downgrading the renderer's wgpu**". That was never
//! measured, and it is wrong. Porting `egui-wgpu` **up** to wgpu 30 is eight mechanical fixes
//! (two renames, four `Option`-wrappings, one write-only buffer API across two sites) — see
//! `vendor/egui-wgpu`, which does exactly that and is compiled and tested in this tree.
//! Nothing had to be downgraded and nothing had to be forked.
//!
//! The cost of the wrong conclusion is worth stating plainly, because it is what this module
//! *is*: a CPU mirror can never be fully accelerated (a system-memory round trip per frame,
//! taken on the render thread) and can never carry **HDR** (`egui::ColorImage` is 8-bit sRGB,
//! so the entire EDR path quantises away at the copy). Those are not tuning problems.
//!
//! **This module is not obsolete.** It remains the right answer for the **plugin inside
//! Ableton**, where the editor does not own its window and a GPU device has no business in the
//! host's process. What it is no longer is *the only* answer: Organon Mind renders its
//! interface directly onto the renderer's device (`ui_layer.rs`), and pays neither cost.
//!
//! # Why a separate mmap, not `Shared`
//!
//! Same reason as [`crate::mind_ring`] and [`crate::audio_ring`], and the rule is worth stating
//! once more because it is the one that keeps `Shared` usable: **`Shared` is a control-rate
//! snapshot with byte-offset compatibility across every saved Ableton set.** A ~0.9 MB frame at
//! 15 Hz is neither control-rate nor small. It gets its own file, and `Shared` gains nothing but
//! the one-bit request that turns this on (`mindview[3]`, a slot #541 Tier 1 already reserved —
//! so no `LAYOUT_VERSION` movement at all). Not a user-facing toggle: the viewport is native to
//! the editor window, so a running editor asks unconditionally.
//!
//! ## ⚠️ Correction (#609): "a running editor asks unconditionally" was doing too much work
//!
//! That sentence was true of an editor. It was not true of the *plugin*, which is what actually
//! stamped the request — `viewport_on` defaulted to `1`, so `process()` published it from the
//! first audio block whether or not an editor had ever existed, and nothing ever wrote `0`. A
//! projector-only session in Ableton paid a second complete 640×360 scene render plus a blocking
//! `poll(Wait)` readback at ~15 Hz, forever, for a viewport nobody had opened.
//!
//! A running editor still asks unconditionally — there is still no toggle. What is new is that a
//! *shut* editor stops asking: the request is now [`mirror_requested`], the conjunction of
//! `EguiState::is_open()` with the pane's own draw-site latch.
//!
//! # Liveness — why a reader starts from `write_seq`, not from zero
//!
//! A quit visual leaves this file behind, and **nothing about it looks dead**: valid magic, right
//! size, a nonzero `write_seq`, and a slot that is perfectly self-consistent *precisely because*
//! nobody is writing to it — so even the torn-read guard passes. A reader that started at
//! `last = 0` would hand that dead process's final frame to the editor as the live scene, and the
//! "open the visual window" placeholder would never appear again on any machine where the visual
//! had run once.
//!
//! So [`FrameRingReader::open_at`] seeds `last` from whatever is already published: a reader only
//! ever serves frames written *after* it started watching. A corpse's `write_seq` never advances;
//! a live writer advances within a frame or two of its ~15 Hz clock.
//!
//! [`crate::ipc::Reader::is_live`] solves the same problem for `Shared` by sleeping up to
//! 6 × 25 ms probing for a `seq` advance. That is right for a one-off startup check and wrong on
//! the editor's paint path. Deleting the file on shutdown would not be enough either: a crash or a
//! kill leaves it behind, which is exactly when a stale picture presented as live misleads most.
//!
//! # The layout
//!
//! Deliberately **not** a `#[derive(Pod)]` struct. A slot is ~0.9 MB, so a `Pod` type containing
//! the pixel array would invite constructing one on the stack — `MindFrame`'s pattern
//! (`bytemuck::bytes_of(&frame)`) is fine at 24 KB and a blown stack at 0.9 MB. Offsets are
//! computed instead, and every write goes straight into the map.
//!
//! ```text
//! header  [0..4]   MAGIC
//!         [4..8]   write_seq        ← published LAST; 0 = nothing written yet
//!         [8..12]  slot_count
//!         [12..16] max_w
//!         [16..20] max_h
//!         [20..32] reserved
//! slot i  at SLOTS + i * SLOT_STRIDE
//!         [0..4]   seq              ← must equal header.write_seq to be trusted
//!         [4..8]   w
//!         [8..12]  h
//!         [12..16] reserved
//!         [16..]   RGBA8 pixels, w*h*4 bytes, tightly packed
//! ```
//!
//! **Newest-wins, drop-don't-stall.** The writer never waits for a reader: it laps. A reader
//! that falls behind loses frames, which for a viewport is exactly right — a stale frame is
//! worthless, and blocking the render thread to deliver one would be actively harmful.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

/// `"omfr"` — a sanity marker so a stale file from another build is ignored rather than decoded.
pub const MAGIC: u32 = 0x6F6D_6672;

/// Header size in bytes.
const HEADER: usize = 32;
/// Per-slot metadata size, before the pixels.
const SLOT_HEADER: usize = 16;
/// Where slot 0 begins.
const SLOTS: usize = HEADER;

/// Frames in flight.
///
/// Three, matching [`crate::mind_ring`]. One would tear constantly; two lets a reader mid-copy
/// be lapped by a writer publishing the next; three gives a full frame of slack, which at the
/// rates involved (a 15 Hz writer against a ~60 Hz editor) is ample. More would only add
/// resident memory for staleness nobody wants.
pub const SLOT_COUNT: usize = 3;

/// Mirror width in pixels.
///
/// **`640` is load-bearing, not arbitrary.** wgpu requires a readback's `bytes_per_row` to be a
/// multiple of 256, and `640 × 4 = 2560 = 256 × 10`. Any width that breaks that forces the
/// visual to copy row-by-row around padding on its way out of the GPU, which is precisely the
/// per-pixel work Tier 3 exists to avoid. Widths that keep the property: multiples of 64.
pub const MIRROR_W: u32 = 640;
/// Mirror height — 16:9 against [`MIRROR_W`].
pub const MIRROR_H: u32 = 360;

/// Bytes of pixel payload a slot can hold.
pub const MAX_PIXELS: usize = (MIRROR_W as usize) * (MIRROR_H as usize) * 4;
/// Stride from one slot to the next.
const SLOT_STRIDE: usize = SLOT_HEADER + MAX_PIXELS;
/// Total file size.
pub const RING_SIZE: usize = SLOTS + SLOT_COUNT * SLOT_STRIDE;

/// `$TMPDIR/<namespace>-frame.bin` — the mirror channel, namespaced like every other IPC file so
/// an Organon session and an Organon Mind session cannot stomp each other.
pub fn frame_ring_path() -> PathBuf {
    crate::ipc::ns_file("frame.bin")
}

/// **Should the visual publish mirror frames?** (#609) — the rule behind `Shared.mindview[3]`,
/// extracted so it can be *tested* rather than asserted in a comment.
///
/// Both halves are load-bearing, and the bug this replaces is what proves it:
///
/// - `editor_open` — `EguiState::is_open()`, set by `nih_plug_egui` in `Editor::spawn` and
///   cleared in `EguiEditorHandle::drop`. **This is the half that was missing.** Without it the
///   plugin published the request from its first audio block and never stopped, so a
///   projector-only session — editor never opened — paid a second complete 640×360 scene render
///   plus a blocking `poll(Wait)` readback at ~15 Hz for the life of the process.
/// - `viewport_drawn` — `viewport_on`, stored where `viewport_pane` is actually drawn. It
///   **latches**: there is no "the pane stopped drawing" event to clear it on, which is exactly
///   why it cannot be the whole answer on its own.
///
/// Read the conjunction as *"an editor is open **and** this build draws a mirror pane"*. The
/// second clause is what lets [#593](https://github.com/organonart/organon/issues/593) Tier 4
/// gate the pane out of the Mind edition and get the request switched off for free, rather than
/// leaving a mirror running for a viewport nobody draws.
///
/// This is deliberately not an inline `&&` at the call site. The whole defect was a claim about
/// when the mirror runs that no test checked — #554 T1's own comment said "off by default" one
/// line above `AtomicU32::new(1)`. A predicate can be wrong; a predicate with tests announces it.
#[inline]
pub fn mirror_requested(editor_open: bool, viewport_drawn: bool) -> bool {
    editor_open && viewport_drawn
}

#[inline]
fn rd_u32(map: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([map[at], map[at + 1], map[at + 2], map[at + 3]])
}

#[inline]
fn wr_u32(map: &mut [u8], at: usize, v: u32) {
    map[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Byte offset of slot `i`.
#[inline]
fn slot_at(i: usize) -> usize {
    SLOTS + i * SLOT_STRIDE
}

// ═════════════════════════════════════════════════════════════════════════════
// Writer — the visual
// ═════════════════════════════════════════════════════════════════════════════

/// The visual's end of the mirror. One per process; nothing else may write the file.
pub struct FrameRingWriter {
    map: memmap2::MmapMut,
    seq: u32,
}

impl FrameRingWriter {
    /// Create (or re-create) the ring, zero it, and stamp the header.
    pub fn create() -> io::Result<FrameRingWriter> {
        Self::create_at(&frame_ring_path())
    }

    /// Create the ring at an explicit path — used by tests, which must not touch the real
    /// channel a running session might be using.
    pub fn create_at(path: &Path) -> io::Result<FrameRingWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(RING_SIZE as u64)?;
        // SAFETY: the file is sized to RING_SIZE above, and this process is the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        // Zero first, so a shorter previous life cannot leave a plausible-looking slot behind.
        map[..RING_SIZE].fill(0);
        wr_u32(&mut map, 0, MAGIC);
        wr_u32(&mut map, 8, SLOT_COUNT as u32);
        wr_u32(&mut map, 12, MIRROR_W);
        wr_u32(&mut map, 16, MIRROR_H);
        // write_seq stays 0: the header is valid, but no frame has been published.
        Ok(FrameRingWriter { map, seq: 0 })
    }

    /// Frames published so far.
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Publish one frame.
    ///
    /// **The store order is the correctness argument**, and it is the same discipline
    /// `mind_ring` uses: fill the slot's pixels, stamp the slot's `seq`, and only *then* publish
    /// `write_seq`. A reader keys on `slot.seq == header.write_seq`, so it can never be pointed
    /// at a slot that is still being filled — the pointer moves after the data, never before.
    ///
    /// Returns `false` and writes nothing if the frame does not fit, rather than truncating: a
    /// half-frame that passes the seq check would be worse than a dropped one.
    pub fn write_frame(&mut self, w: u32, h: u32, rgba: &[u8]) -> bool {
        let need = (w as usize) * (h as usize) * 4;
        if w == 0 || h == 0 || need > MAX_PIXELS || rgba.len() < need {
            return false;
        }
        let next = self.seq.wrapping_add(1);
        // `wrapping_add` can reach 0 after 2^32 frames; 0 means "empty" to a reader, so skip it.
        let next = if next == 0 { 1 } else { next };
        let base = slot_at((next as usize - 1) % SLOT_COUNT);

        // 1. Payload and geometry first — while this slot is still unpublished.
        self.map[base + SLOT_HEADER..base + SLOT_HEADER + need].copy_from_slice(&rgba[..need]);
        wr_u32(&mut self.map, base + 4, w);
        wr_u32(&mut self.map, base + 8, h);
        // 2. Then the slot's own seq, so the slot is internally consistent...
        wr_u32(&mut self.map, base, next);
        // 3. ...and only now publish it as the latest.
        wr_u32(&mut self.map, 4, next);
        self.seq = next;
        true
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Reader — the editor
// ═════════════════════════════════════════════════════════════════════════════

/// The editor's end of the mirror.
///
/// Opening is **total**: a missing, short, or foreign file yields a reader that simply never
/// produces a frame. The editor must work with the visual not running — which is the normal
/// case, not an error — so there is no failure to report and nothing to surface to the user
/// beyond "no frame yet".
pub struct FrameRingReader {
    map: Option<memmap2::Mmap>,
    /// The `seq` most recently handed out, so the caller can skip redundant texture uploads.
    last: u32,
}

impl FrameRingReader {
    /// Open the default channel.
    pub fn open() -> FrameRingReader {
        Self::open_at(&frame_ring_path())
    }

    /// Open a specific path (tests).
    pub fn open_at(path: &Path) -> FrameRingReader {
        let map = OpenOptions::new()
            .read(true)
            .open(path)
            .ok()
            // SAFETY: read-only view; a torn read is caught by the seq check in `take_latest`.
            .and_then(|f| unsafe { memmap2::Mmap::map(&f).ok() })
            .filter(|m: &memmap2::Mmap| m.len() >= RING_SIZE && rd_u32(m, 0) == MAGIC);
        // Start from whatever is ALREADY published, not from 0, so a reader only ever serves
        // frames the writer produced *after* it started watching.
        //
        // Otherwise a quit visual is indistinguishable from a running one: it leaves the ring
        // file in `$TMPDIR` with a valid magic, the right size and a nonzero `write_seq`, so
        // every check above passes. With `last: 0` the first `take_latest` sees
        // `published != last`, the torn-read guard passes (nothing is writing, so the slot is
        // perfectly self-consistent), and the editor draws **a frame from a dead process as if
        // it were live** — while the "open the visual window" placeholder never appears. That
        // placeholder only worked on a machine where the visual had never run.
        //
        // Same corpse-looks-live problem `ipc::Reader::is_live` exists for, fixed differently on
        // purpose: `is_live` sleeps up to 6×25 ms probing for a `seq` advance, which is fine once
        // at startup and not fine on the editor's paint path. Seeding `last` buys the same
        // guarantee for free — a stale file's `write_seq` never advances, so `take_latest`
        // returns `None` forever; a live writer advances within a frame or two of its ~15 Hz
        // clock. Deleting the file on shutdown would NOT be enough on its own: a crash or a kill
        // leaves it behind, which is exactly when a stale picture presented as the live scene is
        // most misleading.
        let last = map.as_ref().map_or(0, |m| rd_u32(m, 4));
        FrameRingReader { map, last }
    }

    /// Is a ring actually present and well-formed?
    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }

    /// The newest published sequence number, or 0 for "nothing yet".
    pub fn published(&self) -> u32 {
        self.map.as_ref().map_or(0, |m| rd_u32(m, 4))
    }

    /// Copy the newest frame into `dst` **if it is newer than the last one taken**.
    ///
    /// Returns `Some((w, h))` on a fresh, intact frame. `None` covers every uninteresting case
    /// together — no ring, nothing published yet, nothing new since last call, or a torn read —
    /// because the caller's response to all four is identical: keep showing what it has.
    ///
    /// # Why the sequence is re-checked *after* the copy
    ///
    /// Checking before would only prove the slot was intact when the copy *started*. The writer
    /// runs free (it must — see the module docs on drop-don't-stall), so it can lap a slow
    /// reader mid-copy and leave `dst` holding two halves of different frames. Re-reading
    /// `write_seq` afterwards and discarding on a mismatch is what makes a lap merely a dropped
    /// frame instead of a visibly torn one. With [`SLOT_COUNT`] = 3 this is rare; "rare" is not
    /// "never", and a torn viewport frame looks like a bug in the renderer.
    pub fn take_latest(&mut self, dst: &mut Vec<u8>) -> Option<(u32, u32)> {
        let map = self.map.as_ref()?;
        let published = rd_u32(map, 4);
        if published == 0 || published == self.last {
            return None;
        }
        let base = slot_at((published as usize - 1) % SLOT_COUNT);
        if rd_u32(map, base) != published {
            return None; // the slot is being refilled right now
        }
        let (w, h) = (rd_u32(map, base + 4), rd_u32(map, base + 8));
        let need = (w as usize) * (h as usize) * 4;
        if w == 0 || h == 0 || need > MAX_PIXELS {
            return None;
        }
        dst.clear();
        dst.extend_from_slice(&map[base + SLOT_HEADER..base + SLOT_HEADER + need]);
        // Re-validate: if either the slot or the ring moved under us, throw the copy away.
        if rd_u32(map, base) != published || rd_u32(map, 4) != published {
            return None;
        }
        self.last = published;
        Some((w, h))
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp path per test, so tests never share a channel.
    fn path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("organon-frame-test-{tag}.bin"))
    }

    /// A frame whose every pixel encodes `fill`, so a torn or stale read is detectable.
    fn frame(fill: u8) -> Vec<u8> {
        vec![fill; MAX_PIXELS]
    }

    #[test]
    fn readback_width_stays_wgpu_row_aligned() {
        // wgpu requires `bytes_per_row % 256 == 0` on a texture→buffer copy. If this ever
        // fails, the visual must copy row-by-row around padding on the way out of the GPU —
        // exactly the per-pixel work the mirror is supposed to avoid. Cheaper to assert than
        // to rediscover as a mystery perf regression.
        assert_eq!((MIRROR_W as usize * 4) % 256, 0, "MIRROR_W must keep rows 256-aligned");
        assert_eq!(MAX_PIXELS, SLOT_STRIDE - SLOT_HEADER);
    }

    /// #609 — the four states of the mirror request, one of which shipped wrong for a month.
    ///
    /// The row that matters is the third: an editor that opened, drew the pane and then closed.
    /// `viewport_on` latches, so it is still `1`; only `EguiState::is_open()` going false
    /// distinguishes "someone is looking" from "someone looked once". Before this, the visual
    /// kept rendering a second scene and stalling on a readback for a window that was gone.
    #[test]
    fn the_mirror_is_requested_only_while_an_editor_is_open_and_drawing_it() {
        // Default-inert (invariant #4): the plugin's first audio block, no editor ever opened.
        assert!(!mirror_requested(false, false), "no editor, no pane ⇒ no mirror");
        // The shipped defect, pinned: `viewport_on` was `1` from `Default`, so this case
        // published the request from the very first `process()` call. It must not.
        assert!(!mirror_requested(false, true), "latched request with no editor ⇒ no mirror");
        // A window with no mirror pane in it — what #593 Tier 4 makes true of Organon Mind.
        assert!(!mirror_requested(true, false), "editor open but no pane drawn ⇒ no mirror");
        // The only case that costs anything, and the one the feature exists for.
        assert!(mirror_requested(true, true), "editor open AND drawing the pane ⇒ mirror on");
    }

    #[test]
    fn a_written_frame_reads_back_intact() {
        let p = path("roundtrip");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        // Reader first, *then* the frame. `open_at` seeds `last` from what is already published,
        // so it only ever serves frames written after it started watching — which is what stops
        // a dead writer's leftovers being drawn as live. This is also the runtime order: the
        // editor's reader is long-lived and the visual publishes into it.
        let mut r = FrameRingReader::open_at(&p);
        assert!(w.write_frame(MIRROR_W, MIRROR_H, &frame(0xAB)));
        let mut buf = Vec::new();
        let (gw, gh) = r.take_latest(&mut buf).expect("a frame");
        assert_eq!((gw, gh), (MIRROR_W, MIRROR_H));
        assert_eq!(buf.len(), MAX_PIXELS);
        assert!(buf.iter().all(|&b| b == 0xAB), "payload survived the round trip");
        let _ = std::fs::remove_file(&p);
    }

    /// The reported #557 bug: a quit visual leaves a perfectly well-formed ring behind.
    ///
    /// Nothing about the file distinguishes a corpse from a live writer — valid magic, right
    /// size, nonzero `write_seq`, and a slot that is entirely self-consistent precisely *because*
    /// nobody is writing to it, so the torn-read guard passes too. A reader that trusted the file
    /// would hand a dead process's last frame to the editor as the live scene, and the "open the
    /// visual window" placeholder would never appear again once the visual had run even once on
    /// the machine.
    #[test]
    fn a_stale_ring_from_a_dead_writer_is_never_served() {
        let p = path("corpse");
        {
            let mut w = FrameRingWriter::create_at(&p).expect("create");
            assert!(w.write_frame(MIRROR_W, MIRROR_H, &frame(0x5A)));
        } // writer dropped — the visual quit. The file stays behind.

        let mut r = FrameRingReader::open_at(&p);
        assert!(r.is_open(), "the leftover file is still well-formed — that is the whole trap");
        assert!(r.published() > 0, "and it still carries a published frame");
        assert!(
            r.take_latest(&mut Vec::new()).is_none(),
            "nothing has been published since we started watching, so there is no live frame to \
             show — the editor must fall through to its placeholder"
        );
        let _ = std::fs::remove_file(&p);
    }

    /// And the other half: seeding must not make a *live* writer invisible. A frame published
    /// after the reader opens is served normally, however much history the file already had.
    #[test]
    fn a_live_writer_is_still_seen_through_a_pre_existing_ring() {
        let p = path("resumed");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        assert!(w.write_frame(MIRROR_W, MIRROR_H, &frame(0x11)));

        let mut r = FrameRingReader::open_at(&p);
        assert!(r.take_latest(&mut Vec::new()).is_none(), "the pre-existing frame is history");

        assert!(w.write_frame(MIRROR_W, MIRROR_H, &frame(0x22)));
        let mut buf = Vec::new();
        assert!(r.take_latest(&mut buf).is_some(), "a frame written after we opened is live");
        assert!(buf.iter().all(|&b| b == 0x22), "and it is the new one, not the stale one");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_fresh_ring_yields_nothing() {
        // The editor's normal startup state: header valid, no frame published. Must be silent,
        // not an error and not a garbage frame decoded out of zeroed bytes.
        let p = path("empty");
        let _w = FrameRingWriter::create_at(&p).expect("create");
        let mut r = FrameRingReader::open_at(&p);
        assert!(r.is_open());
        assert_eq!(r.published(), 0);
        assert!(r.take_latest(&mut Vec::new()).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_missing_or_foreign_file_is_silent() {
        // The editor runs with the visual closed most of the time. That is not an error.
        let mut absent = FrameRingReader::open_at(&path("does-not-exist"));
        assert!(!absent.is_open());
        assert!(absent.take_latest(&mut Vec::new()).is_none());

        // A file of the right size but the wrong magic (a stale build, or something else
        // entirely) must be rejected rather than decoded.
        let p = path("foreign");
        std::fs::write(&p, vec![0x5A; RING_SIZE]).expect("write");
        let mut foreign = FrameRingReader::open_at(&p);
        assert!(!foreign.is_open(), "wrong magic must not be decoded");
        let _ = std::fs::remove_file(&p);

        // A truncated file must be rejected too — otherwise slot reads index out of bounds.
        let p2 = path("short");
        std::fs::write(&p2, vec![0u8; HEADER]).expect("write");
        assert!(!FrameRingReader::open_at(&p2).is_open());
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn only_fresh_frames_are_handed_out() {
        // The editor calls this every repaint (~60 Hz) against a ~15 Hz writer, so most calls
        // must be cheap no-ops. If a stale frame were re-returned, every repaint would re-upload
        // ~0.9 MB to the GPU for no reason.
        let p = path("fresh");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        let mut r = FrameRingReader::open_at(&p);
        let mut buf = Vec::new();

        w.write_frame(MIRROR_W, MIRROR_H, &frame(1));
        assert!(r.take_latest(&mut buf).is_some(), "first frame is new");
        assert!(r.take_latest(&mut buf).is_none(), "same frame must not repeat");
        assert!(r.take_latest(&mut buf).is_none(), "still nothing new");

        w.write_frame(MIRROR_W, MIRROR_H, &frame(2));
        assert!(r.take_latest(&mut buf).is_some(), "a new frame is new");
        assert_eq!(buf[0], 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_lapped_reader_gets_the_newest_not_the_oldest() {
        // Drop-don't-stall: the writer never waits, so a reader that misses frames must resume
        // at the *latest*, never work through a backlog. A viewport showing a queued-up stale
        // frame is worse than one that skipped.
        let p = path("lapped");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        let mut r = FrameRingReader::open_at(&p);
        let mut buf = Vec::new();

        for fill in 1..=(SLOT_COUNT as u8 * 3) {
            w.write_frame(MIRROR_W, MIRROR_H, &frame(fill));
        }
        r.take_latest(&mut buf).expect("a frame");
        assert_eq!(buf[0], SLOT_COUNT as u8 * 3, "resumed at the newest frame");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_slot_being_refilled_is_not_handed_out() {
        // The torn-read guard. Simulated by publishing `write_seq` while the target slot still
        // carries an older `seq` — exactly the window the writer's store order avoids, and the
        // one a reader must survive if it ever does observe it.
        let p = path("torn");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        w.write_frame(MIRROR_W, MIRROR_H, &frame(7));

        // Forge the inconsistency directly in the file.
        {
            let f = OpenOptions::new().read(true).write(true).open(&p).expect("open");
            let mut m = unsafe { memmap2::MmapMut::map_mut(&f).expect("map") };
            wr_u32(&mut m, 4, 99); // header claims seq 99...
            // ...but no slot carries it.
            m.flush().ok();
        }
        let mut r = FrameRingReader::open_at(&p);
        assert!(r.take_latest(&mut Vec::new()).is_none(), "mismatched seq must be rejected");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn an_oversized_or_empty_frame_is_refused_not_truncated() {
        // Truncating would produce a half-frame that passes every downstream check — a corrupt
        // picture rather than a missing one. Refusing keeps the failure visible.
        let p = path("oversize");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        assert!(!w.write_frame(MIRROR_W * 2, MIRROR_H, &vec![1; MAX_PIXELS * 4]), "too big");
        assert!(!w.write_frame(0, MIRROR_H, &frame(1)), "zero width");
        assert!(!w.write_frame(MIRROR_W, 0, &frame(1)), "zero height");
        // A buffer shorter than the declared geometry is a caller bug, not a partial frame.
        assert!(!w.write_frame(MIRROR_W, MIRROR_H, &[1, 2, 3]), "short buffer");
        assert_eq!(w.seq(), 0, "no rejected frame may bump the sequence");
        assert!(FrameRingReader::open_at(&p).take_latest(&mut Vec::new()).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_smaller_frame_than_the_maximum_is_fine() {
        // Tier 3 varies resolution with the pane size; the geometry travels per-frame precisely
        // so that lands as a writer change with no ring-format change.
        let p = path("smaller");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        let (sw, sh) = (320u32, 180u32);
        // Reader before the write — `open_at` serves only frames published *after* it started
        // watching, so a frame written first would (correctly) read as history.
        let mut r = FrameRingReader::open_at(&p);
        assert!(w.write_frame(sw, sh, &vec![0x33; (sw * sh * 4) as usize]));
        let mut buf = Vec::new();
        assert_eq!(r.take_latest(&mut buf), Some((sw, sh)));
        assert_eq!(buf.len(), (sw * sh * 4) as usize, "reader trusts the frame's own geometry");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn recreating_the_ring_does_not_resurrect_an_old_frame() {
        // The visual restarts more often than the editor does. A fresh writer must not leave the
        // editor showing a frame from the previous process.
        let p = path("recreate");
        let mut w = FrameRingWriter::create_at(&p).expect("create");
        w.write_frame(MIRROR_W, MIRROR_H, &frame(0x11));
        drop(w);
        let _w2 = FrameRingWriter::create_at(&p).expect("recreate");
        let mut r = FrameRingReader::open_at(&p);
        assert_eq!(r.published(), 0, "a recreated ring publishes nothing");
        assert!(r.take_latest(&mut Vec::new()).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn the_ring_is_a_bounded_size() {
        // A viewport must not cost unbounded resident memory. Three 640×360 RGBA slots plus
        // headers is ~2.7 MB; assert the order of magnitude so a resolution change is a
        // deliberate decision rather than a silent one.
        assert_eq!(RING_SIZE, HEADER + SLOT_COUNT * (SLOT_HEADER + MAX_PIXELS));
        assert!(RING_SIZE < 4 * 1024 * 1024, "ring grew past 4 MB: {RING_SIZE}");
    }
}
