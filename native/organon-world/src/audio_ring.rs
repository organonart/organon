//! Continuous audio-sample ring (#430 Tier 2).
//!
//! A SEPARATE memory-mapped channel from `Shared`, carrying the plugin's live
//! **post-synth stereo output** (host passthrough + synth — "whatever was flowing")
//! from a single writer (the plugin's `process()`) to a single reader (the visual's
//! in-app recorder), so a recording can carry the music. It is deliberately off
//! `Shared` because audio is a **continuous high-rate stream**, not a control-rate
//! snapshot — so this is a flat circular sample buffer with a monotonic `write_count`,
//! not `Shared`'s whole-struct copy or `mind_ring`'s latest-slot design.
//!
//! Discipline mirrors `mind_ring.rs`: an mmap file, a `signature` guard, and a single
//! writer / single reader. The writer is **always on** while the plugin processes audio
//! (a couple of stores per sample — the audio *output* is byte-identical); the reader
//! only drains while recording. Torn reads of one `f32` sample (or the `write_count`
//! u64) are acoustically negligible at worst, the same control-rate philosophy the rest
//! of the IPC uses.

use bytemuck::{Pod, Zeroable};
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

/// "OAUD" — torn-read / wrong-file guard stamped in the ring header.
pub const AUDIO_RING_SIGNATURE: u32 = 0x4F_41_55_44;
/// Ring capacity in **stereo frames** (power of two for cheap wrap). 2^19 = 524288 ≈
/// 5.4 s @96k / 10.9 s @48k — far more than the visual's per-frame drain interval needs,
/// so it never overruns in practice even if a readback hitches.
pub const AR_FRAMES: usize = 1 << 19;

/// Header at the front of the mmap; the interleaved `f32` L/R samples follow it.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct AudioRingHeader {
    pub signature: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub capacity: u32,
    /// Total stereo frames ever written (monotonic); the live head is at
    /// `write_count % capacity`.
    pub write_count: u64,
    pub _pad: u64,
}

const HEADER_SIZE: usize = std::mem::size_of::<AudioRingHeader>(); // 32
const SAMPLES_BYTES: usize = AR_FRAMES * 2 * 4; // stereo f32
const FILE_SIZE: usize = HEADER_SIZE + SAMPLES_BYTES;
const FRAME_BYTES: usize = 8; // 2 × f32
// Header field byte offsets (repr(C), 8-aligned).
const OFF_WRITE_COUNT: usize = 16;

/// The audio-ring path (`$TMPDIR/organic-math-audio.bin`). Re-exported from `ipc.rs`.
pub fn audio_ring_path() -> PathBuf {
    organon_core::ipc::audio_ring_path()
}

/// Ring writer (the plugin). Created once per `initialize()` with the host sample rate,
/// then `push_frame` per output sample from `process()`.
pub struct AudioRingWriter {
    map: memmap2::MmapMut,
    write_count: u64,
}

impl AudioRingWriter {
    pub fn create(sample_rate: u32) -> io::Result<AudioRingWriter> {
        Self::create_at(&audio_ring_path(), sample_rate)
    }

    /// Create/size the ring file, stamp the header, and reset the write head. The
    /// samples region is left as-is (a fresh file is zero-filled by `set_len`).
    pub fn create_at(path: &Path, sample_rate: u32) -> io::Result<AudioRingWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(FILE_SIZE as u64)?;
        // SAFETY: file is sized to FILE_SIZE; we are the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let hdr = AudioRingHeader {
            signature: AUDIO_RING_SIGNATURE,
            sample_rate,
            channels: 2,
            capacity: AR_FRAMES as u32,
            write_count: 0,
            _pad: 0,
        };
        map[..HEADER_SIZE].copy_from_slice(bytemuck::bytes_of(&hdr));
        Ok(AudioRingWriter { map, write_count: 0 })
    }

    /// Append one stereo frame at the head and publish the new `write_count` (so a
    /// reader that reads the count then the samples never reads past written data).
    #[inline]
    pub fn push_frame(&mut self, l: f32, r: f32) {
        let slot = (self.write_count % AR_FRAMES as u64) as usize;
        let off = HEADER_SIZE + slot * FRAME_BYTES;
        self.map[off..off + 4].copy_from_slice(&l.to_le_bytes());
        self.map[off + 4..off + 8].copy_from_slice(&r.to_le_bytes());
        self.write_count += 1;
        self.map[OFF_WRITE_COUNT..OFF_WRITE_COUNT + 8]
            .copy_from_slice(&self.write_count.to_le_bytes());
    }
}

/// Ring reader (the visual's recorder). Tracks a monotonic frame cursor; `drain` appends
/// every new interleaved sample since the last call.
pub struct AudioRingReader {
    path: PathBuf,
    map: Option<memmap2::Mmap>,
    cursor: u64,
    /// Frames to wait before the next lazy reopen attempt while the ring file is absent
    /// (so a missing writer isn't a per-frame syscall storm).
    reopen_countdown: u32,
}

impl AudioRingReader {
    pub fn open() -> AudioRingReader {
        Self::open_at(&audio_ring_path())
    }

    pub fn open_at(path: &Path) -> AudioRingReader {
        AudioRingReader {
            path: path.to_path_buf(),
            map: Self::try_map(path),
            cursor: 0,
            reopen_countdown: 0,
        }
    }

    fn try_map(path: &Path) -> Option<memmap2::Mmap> {
        OpenOptions::new().read(true).open(path).ok().and_then(|f| {
            if f.metadata().map(|m| m.len() as usize >= FILE_SIZE).unwrap_or(false) {
                // SAFETY: file is at least FILE_SIZE bytes.
                unsafe { memmap2::Mmap::map(&f).ok() }
            } else {
                None
            }
        })
    }

    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }

    fn header(&self) -> Option<AudioRingHeader> {
        let m = self.map.as_ref()?;
        let hdr: AudioRingHeader = bytemuck::pod_read_unaligned(&m[..HEADER_SIZE]);
        (hdr.signature == AUDIO_RING_SIGNATURE).then_some(hdr)
    }

    /// Host sample rate the writer stamped (0 if no valid writer yet).
    pub fn sample_rate(&self) -> u32 {
        self.header().map(|h| h.sample_rate).unwrap_or(0)
    }

    pub fn channels(&self) -> u32 {
        self.header().map(|h| h.channels).unwrap_or(2)
    }

    /// Start draining from the live head — call at record-start so captured audio aligns
    /// to the video, ignoring whatever pre-roll is already in the ring.
    pub fn reset_to_now(&mut self) {
        self.cursor = self.header().map(|h| h.write_count).unwrap_or(0);
    }

    /// Append all new interleaved stereo frames since the cursor into `out`; returns the
    /// number of **frames** appended. If the reader fell behind by more than the ring
    /// capacity (an overrun), it skips to the oldest still-available frame — the dropped
    /// gap is reported by the return being fewer frames than `write_count` advanced.
    pub fn drain(&mut self, out: &mut Vec<f32>) -> u64 {
        // Late writer: the plugin may create the ring AFTER we opened (recording started
        // before the plugin initialized). Retry the map, throttled so a missing file isn't a
        // per-frame syscall storm. A fresh map drains from the ring's start (cursor 0).
        if self.map.is_none() {
            if self.reopen_countdown > 0 {
                self.reopen_countdown -= 1;
                return 0;
            }
            self.reopen_countdown = 30; // ~0.5s between attempts at 60fps
            self.map = Self::try_map(&self.path);
            if self.map.is_none() {
                return 0;
            }
            self.cursor = 0;
        }
        let Some(m) = self.map.as_ref() else { return 0 };
        let hdr: AudioRingHeader = bytemuck::pod_read_unaligned(&m[..HEADER_SIZE]);
        if hdr.signature != AUDIO_RING_SIGNATURE {
            return 0;
        }
        let w = hdr.write_count;
        // The ring was recreated (plugin re-initialized → write_count reset below our
        // cursor): re-anchor to the new ring's start rather than stalling forever.
        if w < self.cursor {
            self.cursor = 0;
        }
        if w <= self.cursor {
            return 0;
        }
        let cap = AR_FRAMES as u64;
        let start = if w - self.cursor > cap { w - cap } else { self.cursor };
        out.reserve(((w - start) * 2) as usize);
        let mut i = start;
        while i < w {
            let off = HEADER_SIZE + (i % cap) as usize * FRAME_BYTES;
            let l = f32::from_le_bytes([m[off], m[off + 1], m[off + 2], m[off + 3]]);
            let r = f32::from_le_bytes([m[off + 4], m[off + 5], m[off + 6], m[off + 7]]);
            out.push(l);
            out.push(r);
            i += 1;
        }
        self.cursor = w;
        w - start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("organic-math-audio-test-{}-{}.bin", std::process::id(), tag))
    }

    #[test]
    fn round_trip_drains_new_frames() {
        let path = tmp_path("rt");
        let _ = std::fs::remove_file(&path);
        let mut w = AudioRingWriter::create_at(&path, 48_000).unwrap();
        for i in 0..5 {
            w.push_frame(i as f32, -(i as f32));
        }
        let mut r = AudioRingReader::open_at(&path);
        assert_eq!(r.sample_rate(), 48_000);
        assert_eq!(r.channels(), 2);
        let mut out = Vec::new();
        let n = r.drain(&mut out);
        assert_eq!(n, 5);
        assert_eq!(out, vec![0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0, 4.0, -4.0]);
        // A second drain with no new writes yields nothing.
        out.clear();
        assert_eq!(r.drain(&mut out), 0);
        assert!(out.is_empty());
        // More writes → only the delta drains.
        w.push_frame(5.0, -5.0);
        assert_eq!(r.drain(&mut out), 1);
        assert_eq!(out, vec![5.0, -5.0]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reset_to_now_skips_preroll() {
        let path = tmp_path("reset");
        let _ = std::fs::remove_file(&path);
        let mut w = AudioRingWriter::create_at(&path, 96_000).unwrap();
        for i in 0..10 {
            w.push_frame(i as f32, i as f32);
        }
        let mut r = AudioRingReader::open_at(&path);
        r.reset_to_now(); // ignore the 10 pre-roll frames
        let mut out = Vec::new();
        assert_eq!(r.drain(&mut out), 0);
        w.push_frame(100.0, 100.0);
        assert_eq!(r.drain(&mut out), 1);
        assert_eq!(out, vec![100.0, 100.0]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wraps_around_capacity() {
        let path = tmp_path("wrap");
        let _ = std::fs::remove_file(&path);
        let mut w = AudioRingWriter::create_at(&path, 48_000).unwrap();
        // Write just over one full lap so the head wraps; the reader keeping up sees all.
        let total = AR_FRAMES as u64 + 3;
        let mut r = AudioRingReader::open_at(&path);
        let mut out = Vec::new();
        let mut got = 0u64;
        // Drain in chunks so we never fall behind (simulates the visual draining per frame).
        for i in 0..total {
            w.push_frame(i as f32, i as f32);
            if i % 1000 == 0 {
                got += r.drain(&mut out);
                out.clear();
            }
        }
        got += r.drain(&mut out);
        assert_eq!(got, total, "a keeping-up reader sees every frame across a wrap");
        // The very last frame's value survived the wrap.
        assert_eq!(*out.last().unwrap(), (total - 1) as f32);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn overrun_skips_the_lost_gap() {
        let path = tmp_path("overrun");
        let _ = std::fs::remove_file(&path);
        let mut w = AudioRingWriter::create_at(&path, 48_000).unwrap();
        // Write more than capacity WITHOUT draining → the reader fell behind; it should
        // skip to the oldest still-available frame rather than read stale/garbage.
        let total = AR_FRAMES as u64 + 100;
        for i in 0..total {
            w.push_frame(i as f32, i as f32);
        }
        let mut r = AudioRingReader::open_at(&path); // cursor = 0, way behind
        let mut out = Vec::new();
        let n = r.drain(&mut out);
        assert_eq!(n, AR_FRAMES as u64, "capped at capacity — the gap is dropped");
        // The newest frame is present and correct.
        assert_eq!(*out.last().unwrap(), (total - 1) as f32);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_inert() {
        let missing = tmp_path("missing");
        let _ = std::fs::remove_file(&missing);
        let mut r = AudioRingReader::open_at(&missing);
        assert!(!r.is_open());
        assert_eq!(r.sample_rate(), 0);
        let mut out = Vec::new();
        assert_eq!(r.drain(&mut out), 0);
    }

    #[test]
    fn reanchors_when_ring_recreated() {
        // Bugbot: plugin re-initializes mid-take → write_count resets to 0, below the
        // reader's cursor. `drain` must re-anchor instead of stalling forever.
        let path = tmp_path("recreate");
        let _ = std::fs::remove_file(&path);
        let mut w = AudioRingWriter::create_at(&path, 48_000).unwrap();
        for i in 0..100 {
            w.push_frame(i as f32, i as f32);
        }
        let mut r = AudioRingReader::open_at(&path);
        r.reset_to_now(); // cursor = 100
        // Recreate the ring (a second initialize()): header write_count back to 0.
        let mut w2 = AudioRingWriter::create_at(&path, 48_000).unwrap();
        w2.push_frame(7.0, 7.0);
        let mut out = Vec::new();
        let n = r.drain(&mut out);
        assert_eq!(n, 1, "re-anchored to the recreated ring rather than stalling");
        assert_eq!(out, vec![7.0, 7.0]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lazy_reopen_after_late_writer() {
        // Bugbot: reader opened before the file existed must reopen once the plugin creates
        // the ring, not stay inert for the whole take.
        let path = tmp_path("late");
        let _ = std::fs::remove_file(&path);
        let mut r = AudioRingReader::open_at(&path);
        assert!(!r.is_open());
        let mut out = Vec::new();
        assert_eq!(r.drain(&mut out), 0); // no file yet
        let mut w = AudioRingWriter::create_at(&path, 48_000).unwrap();
        w.push_frame(1.0, 2.0);
        // Exhaust the reopen throttle; then it maps and captures.
        let mut got = 0;
        for _ in 0..64 {
            got += r.drain(&mut out);
            if got > 0 {
                break;
            }
        }
        assert!(r.is_open(), "reader reopened once the ring appeared");
        assert_eq!(got, 1);
        assert_eq!(out, vec![1.0, 2.0]);
        let _ = std::fs::remove_file(&path);
    }
}
