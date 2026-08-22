//! **The shared mapping, and the only `unsafe` in this crate.**
//!
//! One type, [`Mapping`], holding a file and its `MmapMut`, plus the four accessors every
//! other module goes through. Everything unsound-looking in `organon-module` is here, so the
//! reasoning is in one place rather than repeated at forty call sites.
//!
//! # Why the accessors take `&self` and hand back mutable views
//!
//! A shared mapping is memory **another process writes**. Rust's aliasing model has nothing
//! to say about that: there is no `&mut` that excludes a different process, and no `&` whose
//! immutability another process respects. Pretending otherwise by threading `&mut Mapping`
//! around would buy an appearance of soundness and cost real clarity, because the compiler's
//! guarantee would be describing something it cannot see.
//!
//! So the discipline is the one `organon-core::ipc` already runs on, stated rather than
//! implied, and it has exactly two rules:
//!
//! 1. 🚨 **Every word both parties touch is an atomic**, accessed only through
//!    [`Mapping::u32_at`] / [`Mapping::u64_at`]. A `fence` orders atomic accesses against
//!    each other and establishes nothing about ordinary loads and stores, so bracketing a
//!    plain `[u8]` copy with fences is formally a data race however reliably it behaves on
//!    x86-64 and ARM64. `ipc.rs`'s `seq_cell` carries the same note and the same fix.
//! 2. 🚨 **Every byte range that is not an atomic has exactly one writer**, and the block
//!    layout in [`crate::wire`] is what assigns them: the console owns the control block and
//!    the input ring's head, the producer owns the status block, the input ring's tail and
//!    every slot body. A seqlock brackets each of those bodies so a *reader* can tell it
//!    caught one mid-write, which is the only race the layout deliberately permits.
//!
//! ⚠️ **Rule 2 is a rule about US, not a guarantee about the file.** A hostile or simply
//! buggy producer can write anywhere in the mapping, and nothing here can stop it — the
//! process boundary bounds what a module reaches *through the protocol*, not what it reaches
//! *on the machine*, which is §5.2's asymmetry arriving one layer down. What the consumer
//! side does about it is [`crate::ring`]'s job: it validates every field it reads out of a
//! slot header rather than trusting it, and it re-checks the seqlock after copying so a slot
//! rewritten under it is **discarded** rather than painted.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64};

/// A memory-mapped file, shared with exactly one other process.
pub(crate) struct Mapping {
    /// Kept alive because the mapping is only valid while the file is open on some
    /// platforms, and because dropping it is how the descriptor is released.
    _file: File,
    map: memmap2::MmapMut,
    ptr: *mut u8,
    len: usize,
}

// SAFETY: `ptr` is the mapping's own base and is valid for `len` bytes for as long as `map`
// lives, which is as long as `Mapping` lives. `MmapMut` is `Send`; the raw pointer is the only
// reason the derive would not apply, and it points at nothing thread-local. `Mapping` is
// deliberately **not** `Sync`: one owner drives it, and the cross-*process* sharing is what the
// atomics are for.
unsafe impl Send for Mapping {}

impl Mapping {
    /// Create (or truncate to) a file of exactly `bytes` and map it read/write.
    ///
    /// ⚠️ **The file is truncated to length on purpose.** A channel file left behind by a
    /// previous run holds a previous run's header, a previous run's slot counters and a
    /// previous run's stale "latest" pointer. Resizing to the same length would leave every
    /// one of those in place — and the console's `create` is precisely the moment nothing may
    /// be inherited. `organon-core::ipc::Writer` resumes its counter for the opposite reason
    /// (a live reader may already be attached to a channel that is being re-initialised); a
    /// module channel is created before its producer exists, so there is nobody to be
    /// consistent with.
    pub(crate) fn create(path: &Path, bytes: u64) -> io::Result<Mapping> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(bytes)?;
        // SAFETY: the file has just been sized to `bytes`.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let ptr = map.as_mut_ptr();
        let len = map.len();
        Ok(Mapping { _file: file, map, ptr, len })
    }

    /// Open an existing channel file read/write.
    ///
    /// The producer needs write access even though it "only" produces: it writes the status
    /// block, the input ring's tail and every slot body.
    pub(crate) fn open(path: &Path) -> io::Result<Mapping> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        // SAFETY: whatever length the file has, that is the length of the mapping; every
        // accessor on this type is bounds-checked against `len`, and the header is validated
        // against it before any offset from it is used.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let ptr = map.as_mut_ptr();
        let len = map.len();
        Ok(Mapping { _file: file, map, ptr, len })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The first `n` bytes, for header parsing. Plain, because the header is written once
    /// before the producer exists and is read-only thereafter.
    pub(crate) fn head(&self, n: usize) -> &[u8] {
        &self.map[..n.min(self.len)]
    }

    /// A 4-byte word as a real `AtomicU32`.
    ///
    /// # Panics
    /// If `off` is out of range or not 4-aligned. Both are programming errors in this crate
    /// rather than conditions a file can be in: every offset used here is derived from a
    /// [`crate::wire::Header`] that [`crate::wire::Header::read`] has already checked fits
    /// inside the mapping, and the block offsets are alignment-tested in `wire`.
    #[inline]
    pub(crate) fn u32_at(&self, off: u64) -> &AtomicU32 {
        let off = off as usize;
        assert!(off + 4 <= self.len && off.is_multiple_of(4), "u32 at {off} is outside or misaligned");
        // SAFETY: bounds and alignment just checked; the pointer is the mapping's base.
        unsafe { &*(self.ptr.add(off) as *const AtomicU32) }
    }

    /// An 8-byte word as a real `AtomicU64`. Same panics, same reasoning.
    #[inline]
    pub(crate) fn u64_at(&self, off: u64) -> &AtomicU64 {
        let off = off as usize;
        assert!(off + 8 <= self.len && off.is_multiple_of(8), "u64 at {off} is outside or misaligned");
        // SAFETY: bounds and alignment just checked; the pointer is the mapping's base.
        unsafe { &*(self.ptr.add(off) as *const AtomicU64) }
    }

    /// A byte range, for reading a body a seqlock brackets.
    ///
    /// # Panics
    /// If the range leaves the mapping.
    #[inline]
    pub(crate) fn bytes(&self, off: u64, len: usize) -> &[u8] {
        let off = off as usize;
        assert!(off + len <= self.len, "range {off}..{} is outside the mapping", off + len);
        // SAFETY: bounds just checked.
        unsafe { std::slice::from_raw_parts(self.ptr.add(off), len) }
    }

    /// A byte range to write into.
    ///
    /// # Safety
    /// The caller must be the sole writer of this range — see rule 2 in the module docs — and
    /// must hold it only for as long as the surrounding seqlock is open. Taking `&self`
    /// rather than `&mut self` is the module docs' subject and is deliberate: a `&mut` here
    /// would claim an exclusivity no `&mut` can have over a shared mapping.
    ///
    /// # Panics
    /// If the range leaves the mapping.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn bytes_mut(&self, off: u64, len: usize) -> &mut [u8] {
        let off = off as usize;
        assert!(off + len <= self.len, "range {off}..{} is outside the mapping", off + len);
        // SAFETY: bounds just checked; the caller carries the sole-writer obligation.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.add(off), len) }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A tiny named temp file that removes itself. The crate has no `tempfile` dependency and
    /// does not want one for this — see the manifest's rule that every edge earns its line.
    pub(crate) struct TempPath(pub std::path::PathBuf);

    impl TempPath {
        pub(crate) fn new(tag: &str) -> TempPath {
            // A counter rather than a clock: two files created inside the same millisecond
            // would collide, and this runs inside a test harness that is deliberately
            // parallel.
            use std::sync::atomic::AtomicU64;
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            TempPath(std::env::temp_dir().join(format!("organon-module-test-{tag}-{pid}-{n}.bin")))
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn create_sizes_the_file_and_zeroes_it() {
        let p = TempPath::new("create");
        std::fs::write(&p.0, vec![0xAB; 4096]).unwrap();
        let m = Mapping::create(&p.0, 1024).unwrap();
        assert_eq!(m.len(), 1024);
        assert!(m.bytes(0, 1024).iter().all(|&b| b == 0), "a stale channel is never inherited");
    }

    #[test]
    fn two_mappings_of_one_file_see_each_others_writes() {
        let p = TempPath::new("shared");
        let a = Mapping::create(&p.0, 256).unwrap();
        let b = Mapping::open(&p.0).unwrap();
        a.u64_at(8).store(0x1234_5678_9abc_def0, Ordering::Release);
        assert_eq!(b.u64_at(8).load(Ordering::Acquire), 0x1234_5678_9abc_def0);
        // SAFETY: this test is the sole writer of 16..32.
        unsafe { a.bytes_mut(16, 4) }.copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(b.bytes(16, 4), &[1, 2, 3, 4]);
    }

    #[test]
    #[should_panic(expected = "outside")]
    fn a_range_off_the_end_panics_rather_than_reading_the_heap() {
        let p = TempPath::new("oob");
        let m = Mapping::create(&p.0, 256).unwrap();
        let _ = m.bytes(240, 32);
    }

    #[test]
    #[should_panic(expected = "misaligned")]
    fn a_misaligned_atomic_panics_rather_than_tearing() {
        let p = TempPath::new("align");
        let m = Mapping::create(&p.0, 256).unwrap();
        let _ = m.u64_at(4);
    }
}
