//! **The hosted-module contract — one sentence, and everything it takes to keep it.**
//!
//! `CONSOLE_ARCHITECTURE.md` §1.14:
//!
//! > **A producer yields a texture the console can sample, at a size the console asks for.**
//!
//! `doc/organon_module_viewport.md` §9's **T2**. Both trees depend on this crate: Organon's
//! console to host a module, `organonart/ascent` to be one. Nothing above it knows what a
//! module *is* — §4.6's *"never: what the module is"* — and nothing in it knows what Organon
//! is either, which is the same rule read from the other side and the reason a game in
//! another repository can link it without acquiring Organon's vocabulary.
//!
//! # Why `organon-module`, and the near-collision it is worth naming
//!
//! The tree's crates are `organon-<one word>` and each word names *the thing the crate is*:
//! `-core` the host-free spine, `-render` the renderer, `-console` the workstation. This one
//! is the **module protocol** — the thing you depend on in order to be, or to host, a module —
//! so `organon-module` is the word.
//!
//! ⚠️ **It sits one letter from `organon-console::module`, and that is worth being explicit
//! about rather than hoping nobody notices.** They are complementary halves of one story and
//! neither is the other:
//!
//! | | `organon-console::module` (T3a) | **`organon-module`** (this crate) |
//! |---|---|---|
//! | Answers | *may this module run?* | *how does it talk once it does?* |
//! | Holds | `organon-module.toml`, `modules.json`, grants, the commit | the mapping, the frame ring, the input ring, the lifecycle |
//! | Lives | inside the console, private to it | in both repositories |
//! | Trust | the console's own record | a channel between two parties, one of them not ours |
//!
//! 📌 Two things make the name the right one rather than merely tolerable. The manifest a
//! module writes is literally called `organon-module.toml`, so *"depend on `organon-module`
//! and write an `organon-module.toml`"* is one idea with one name. And if the manifest types
//! ever want to move — a module author cannot see `organon-console::module`, and one day may
//! need to — this is where they would land, so the name is already pointing at where the seam
//! is likely to end up.
//!
//! # The dependency posture, and the one question it turns on
//!
//! 🚨 **`MIT OR Apache-2.0`, one dependency (`memmap2`), and `cargo tree` is the acceptance
//! test in BOTH trees.** Ascent's invariant 3 forbids an edge to `organon-visual` or
//! `organic-math-native`; a single transitive arrow from here would break a second
//! repository's licence posture rather than ours, which is a stronger obligation than the
//! no-`nih_plug` bar every other permissive crate here carries.
//!
//! **May the contract depend on `wgpu` at all?** Both ends do, so the question is real. The
//! answer is **no, and the helpers are behind a feature**, for three reasons that get worse in
//! order:
//!
//! 1. A wgpu type on the wire ties two *independently built* binaries to one wgpu version, at
//!    which point "the protocol" is really "the same `Cargo.lock` on both sides" — which is
//!    the one thing hosting is supposed to make unnecessary.
//! 2. `wgpu::TextureFormat` has no stable numeric representation, so `as u32` is a value that
//!    changes when upstream inserts a variant. Silently, and in a direction that reads as
//!    corruption. [`PixelFormat`] exists for exactly that.
//! 3. Requiring wgpu would say a producer must be a wgpu program. The contract is *pixels at
//!    a size*, and a producer that renders some other way is a producer this protocol has no
//!    business turning away.
//!
//! So `cargo tree -p organon-module` (default) is `memmap2` and nothing else, and the two
//! preallocated GPU halves — [`FrameTexture`], [`FrameReadback`] — live behind the `wgpu`
//! feature for the callers that already have it.
//!
//! # The five questions this design had to answer
//!
//! Each is answered where it lives, and this is the index.
//!
//! ### 1. What is in a frame header, and how does a consumer know a frame is complete?
//!
//! [`crate::wire::slot`] is the header; [`crate::ring`]'s module docs are the mechanism. In
//! short: a **per-slot seqlock** (odd while writing), `latest_slot` stamped **only after** it
//! closes, and a **reader's hold** the producer never writes over — with the consumer copying
//! into its own buffer and **re-checking the counter afterwards**, so a slot rewritten under
//! it is discarded rather than painted. Tearing is invisible when it works, so it is not left
//! to inspection: `channel_tests` stages the exact interleaving deterministically through
//! [`ModuleChannel::poll_interfered`].
//!
//! ### 2. What happens when the producer dies mid-frame, and how is dead told from slow?
//!
//! A producer that dies mid-frame leaves an odd counter and half a picture in a slot that
//! `latest_slot` does not name — so the frame path never sees it and goes on serving the last
//! whole frame. The *liveness* path is where the death shows: [`crate::presence`] weighs the
//! producer's own claim against a **counter bumped once per producer loop, not once per
//! frame** (which is what tells *paused* from *hung*, and matters because paused is the state
//! a module arrives in) and a clock. 🚨 The honest limit is written down there: **hung and
//! exited-without-a-farewell are not distinguishable from inside a shared mapping**, and the
//! thing that genuinely knows is the process handle, which is T3b's launcher's.
//!
//! §4.6's *never the last good frame* is structural rather than a rule: [`Poll::Frame`] is
//! unreachable once the producer is judged stalled, lost or gone, and [`Present`] turns the
//! verdict into the one instruction the caller's texture obeys.
//!
//! ### 3. Which side owns the size?
//!
//! Three sentences, in [`crate::channel`]'s module docs. The console owns the **capacity**,
//! once. The console **asks**, with no deadline. The producer **answers per frame**, and the
//! frame is the truth — so frames in flight when the request changed are perfectly good
//! pictures rather than something to flush.
//!
//! ### 4. What does input carry, and what does it refuse to carry?
//!
//! [`crate::input`], whose table of refusals is the more load-bearing half of the file.
//! Four verbs, because Ascent's own `InputEvent` is four. No text, no absolute pointer, no
//! clipboard, no paths, no OS handles, no audio — and 🚨 **no generic message**, which is the
//! one addition that would make every future verb free, which is to say ungranted for ever.
//! §5.3's way out is [`crate::input::RESERVED`], refused at the encode site so a console
//! cannot leak it by forgetting.
//!
//! ### 5. Versioning
//!
//! [`crate::wire`]'s module docs. [`wire::MAGIC`] at byte 0 and [`wire::WIRE_VERSION`] at byte
//! 8 are the only permanent positional commitments; everything after them belongs to the
//! version. A mismatch is [`wire::WireFault::VersionMismatch`] naming both numbers — **a
//! legible refusal, never a garbled picture**.
//!
//! # 🚨 What this crate does NOT do, and must not grow into
//!
//! - **It does not launch anything.** No clone, no `cargo build`, no `CreateProcess`. T3b.
//! - **It does not decide whether a module may run.** `organon-console::module` does.
//! - **It does not name a producer, a region, a viewport or a content word.** T4 owns the
//!   vocabulary, and `region.rs` has a standing objection to inventing a `Producer` enum
//!   before there are two.
//! - **It has no audio path**, and §5.2's asymmetry is stated rather than papered over: a
//!   separate process can open WASAPI itself and Ascent already does. The grant is
//!   **honoured, not enforced**.
//! - **It is not mechanism A.** T0 says the copy is affordable — 0.44 ms round trip at
//!   1280×720, 2.6 % of a 16.7 ms frame — so a shared GPU texture is *not yet justified*,
//!   which is the standard §4.4 set for buying `unsafe` per-backend interop. When it is, it
//!   goes behind this same API.
//!
//! # ⚠️ The number this crate still does not have
//!
//! §4.4's number 2 — **frames of latency between "the module drew it" and "the console
//! painted it"** — is not measured here and nothing in this crate estimates it. Both halves of
//! the test suite live in one process, which proves correctness and says nothing about
//! staleness.
//!
//! ✏️ **What has changed is that it is now a subtraction rather than a research project.**
//! Every frame carries the producer's `SystemTime` at publish, and [`FrameView::age`] is the
//! difference — so the moment two processes exist, the number can be *taken* rather than
//! reasoned about. That was the thing the design said could not be done until T1 and T2
//! existed. They now do.

#![forbid(unsafe_op_in_unsafe_fn)]

mod channel;
mod input;
mod map;
mod presence;
mod ring;
pub mod wire;

#[cfg(feature = "wgpu")]
pub mod gpu;

#[cfg(any(test, feature = "sim"))]
pub mod sim;

pub use channel::{
    channel_file_name, Control, ModuleChannel, OpenFault, ProducerChannel, ProducerStatus,
};
pub use input::{Button, DrainReport, InputEvent, Key, MouseButton, PushFault, RESERVED};
pub use presence::{Poll, Presence, Present, Timings};
pub use ring::{FrameView, FrameWrite};
pub use wire::{
    FrameCapacity, Header, Lifecycle, PixelFormat, ProducerState, RefusalReason, WireFault, MAGIC,
    SLOTS,
    WIRE_VERSION,
};

#[cfg(feature = "wgpu")]
pub use gpu::{FrameReadback, FrameTexture, ReadbackFault};

#[cfg(test)]
mod crate_tests {
    use super::*;

    /// The public surface is small on purpose, and the two halves are named for whose they
    /// are. A test that merely constructs them is worth its four lines: it is what fails when
    /// a re-export is dropped in a refactor, which is otherwise only discovered by the other
    /// repository.
    #[test]
    fn the_two_halves_are_reachable_by_their_public_names() {
        let path = crate::map::tests::TempPath::new("surface");
        let cap = FrameCapacity::new(16, 16, PixelFormat::Rgba8UnormSrgb);
        let console = ModuleChannel::create(&path.0, cap).unwrap();
        assert_eq!(console.capacity(), cap);
        assert_eq!(console.lifecycle(), Lifecycle::Attached);
        let producer = ProducerChannel::open(&path.0).unwrap();
        assert_eq!(producer.control().lifecycle, Lifecycle::Attached);
        assert_eq!(producer.header().wire_version, WIRE_VERSION);
    }

    /// A channel's file name must be a plain component the console can hang under
    /// `ipc::ns_file`'s directory — never a path, never something a producer name could turn
    /// into one. `check_producer_name` in `organon-console::module` already refuses
    /// whitespace and control characters; this is the half that lives on this side.
    #[test]
    fn a_channel_file_name_is_one_component() {
        let n = channel_file_name("ascent", 0);
        assert_eq!(n, "module-ascent-0.frames");
        assert!(!n.contains('/') && !n.contains('\\'));
        assert_ne!(channel_file_name("ascent", 0), channel_file_name("ascent", 1));
    }
}
