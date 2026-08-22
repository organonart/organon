//! **The contract, exercised end to end by two halves of one process.**
//!
//! ⚠️ **This is not §4.4's number 2 and nothing here should be read as it.** Both halves live
//! in one process and one thread, so what these tests prove is *correctness* — that a whole
//! frame arrives whole, that a torn one never does, that a dead producer never yields a
//! picture — not *staleness*. The cross-process latency number still needs two processes;
//! what has changed is that the instrument for taking it now exists
//! ([`crate::FrameView::age`]), so it is a subtraction rather than a research project.
//!
//! 📌 **One thread is a feature of these tests, not a limitation.** A threaded test of a tear
//! detector proves nothing on a good day and flakes on a bad one, because the interleaving it
//! needs is the one the protocol is designed to make rare.
//! [`crate::ModuleChannel::poll_interfered`] stages the exact interleaving deterministically,
//! which is the only way to test a mechanism that is silent whenever it works.

use std::time::{Duration, Instant};

use super::*;
use crate::map::tests::TempPath;
use crate::input::{Button, Key, MouseButton};
use crate::presence::Present;
use crate::sim::{self, SimProducer};
use crate::wire::{PixelFormat, WIRE_VERSION};

/// Fast enough that a whole lifecycle fits in a test, and every threshold is still ordered
/// the way the real ones are.
const QUICK: Timings = Timings {
    start_within: Duration::from_millis(200),
    stall_after: Duration::from_millis(50),
    dead_after: Duration::from_millis(100),
};

fn cap(w: u32, h: u32) -> FrameCapacity {
    FrameCapacity::new(w, h, PixelFormat::Rgba8UnormSrgb)
}

/// A console and a module attached to the same file, plus the temp path that removes itself.
fn pair(tag: &str, capacity: FrameCapacity) -> (TempPath, ModuleChannel, SimProducer) {
    let path = TempPath::new(tag);
    let console = ModuleChannel::create(&path.0, capacity).unwrap().with_timings(QUICK);
    let producer = SimProducer::new(ProducerChannel::open(&path.0).unwrap());
    (path, console, producer)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// ---------------------------------------------------------------------------------------
// The frame path
// ---------------------------------------------------------------------------------------

#[test]
fn a_frame_arrives_whole_and_every_pixel_is_from_the_same_frame() {
    let (_p, mut console, mut producer) = pair("whole", cap(64, 48));
    producer.tick();
    producer.draw().unwrap();

    let now = Instant::now();
    match console.poll(now) {
        Poll::Frame(view) => {
            assert_eq!((view.width, view.height), (64, 48));
            assert_eq!(view.frame_index, 1);
            assert_eq!(view.format, PixelFormat::Rgba8UnormSrgb);
            assert!(view.row_stride >= 64 * 4);
            sim::verify(&view).unwrap();
        }
        other => panic!("expected a frame, got {other:?}"),
    }
    assert_eq!(console.torn_reads(), 0);
    assert_eq!(console.corrupt_reads(), 0);
}

#[test]
fn the_same_frame_is_never_served_twice() {
    let (_p, mut console, mut producer) = pair("once", cap(32, 32));
    producer.tick();
    producer.draw().unwrap();
    let now = Instant::now();
    assert!(matches!(console.poll(now), Poll::Frame(_)));
    // A console polling faster than the producer draws must be told "nothing new", not handed
    // the same bytes to re-upload — that would double the console's half of T0's number for
    // no picture.
    assert!(matches!(console.poll(now), Poll::Holding));
    assert_eq!(console.poll(now).present(), Present::Keep);
}

#[test]
fn a_channel_with_no_frames_yet_says_starting_rather_than_showing_slot_zero() {
    // A freshly zeroed file would otherwise advertise slot 0 as the newest frame, which is a
    // black picture presented as a real one.
    let (_p, mut console, mut producer) = pair("empty", cap(32, 32));
    producer.tick();
    let now = Instant::now();
    assert!(matches!(console.poll(now), Poll::Starting { .. }));
    assert_eq!(console.poll(now).present(), Present::Keep);
}

#[test]
fn frames_keep_flowing_across_many_polls_and_the_index_only_ascends() {
    let (_p, mut console, mut producer) = pair("flow", cap(48, 32));
    let now = Instant::now();
    let mut last = 0;
    for _ in 0..30 {
        producer.tick();
        producer.draw().unwrap();
        match console.poll(now) {
            Poll::Frame(v) => {
                assert!(v.frame_index > last, "{} did not follow {last}", v.frame_index);
                last = v.frame_index;
                sim::verify(&v).unwrap();
            }
            other => panic!("expected a frame, got {other:?}"),
        }
    }
    assert_eq!(last, 30);
    assert_eq!(console.torn_reads(), 0);
}

// ---------------------------------------------------------------------------------------
// Tearing — the failure the ring exists to prevent, and it is invisible when it works
// ---------------------------------------------------------------------------------------

#[test]
fn a_producer_that_ignores_the_hold_is_caught_not_painted() {
    let (_p, mut console, mut producer) = pair("tear", cap(64, 64));
    producer.tick();
    producer.draw().unwrap();
    let now = Instant::now();
    assert!(matches!(console.poll(now), Poll::Frame(_)), "a good frame first");

    producer.draw().unwrap();
    let slot = console
        .map
        .u32_at(console.header.status_off + crate::wire::status::LATEST_SLOT as u64)
        .load(std::sync::atomic::Ordering::Acquire);

    // The producer writes the slot the consumer is holding — the one thing a well-behaved
    // producer never does. `scribble_over` exists only under `cfg(test)` and the `sim`
    // feature, because a shipped build has no business being able to do it.
    let outcome = console.poll_interfered(now, &mut || producer.channel().scribble_over(slot, 0x7E));
    assert!(
        matches!(outcome, Poll::Holding),
        "a blended frame must never be handed over; got {outcome:?}"
    );
    assert_eq!(outcome.present(), Present::Keep, "and the last good picture stays up");
    assert!(console.torn_reads() > 0, "the detector has to actually fire");

    // And the channel recovers: the next honestly-published frame arrives whole.
    producer.draw().unwrap();
    match console.poll(now) {
        Poll::Frame(v) => sim::verify(&v).unwrap(),
        other => panic!("expected recovery, got {other:?}"),
    }
}

#[test]
fn a_producer_that_dies_mid_frame_leaves_the_last_whole_frame_standing() {
    let (_p, mut console, mut producer) = pair("abandon", cap(40, 40));
    producer.tick();
    producer.draw().unwrap();
    let now = Instant::now();
    let first = match console.poll(now) {
        Poll::Frame(v) => {
            sim::verify(&v).unwrap();
            v.frame_index
        }
        other => panic!("expected a frame, got {other:?}"),
    };

    // The pen drops mid-stroke: a slot half full of a picture, its seqlock left odd for ever.
    producer.abandon_frame(40, 40);

    // The frame path sees nothing at all — no tear, no corruption, no half-frame.
    assert!(matches!(console.poll(now), Poll::Holding));
    assert_eq!(console.torn_reads(), 0);
    assert_eq!(console.corrupt_reads(), 0);

    // And the slot is reusable afterwards, because `begin` always moves the counter to the
    // NEXT odd value rather than assuming it starts even.
    producer.draw().unwrap();
    match console.poll(now) {
        Poll::Frame(v) => {
            assert!(v.frame_index > first);
            sim::verify(&v).unwrap();
        }
        other => panic!("expected the next frame, got {other:?}"),
    }
}

#[test]
fn three_slots_always_leave_the_producer_somewhere_to_write() {
    // The argument for SLOTS = 3, checked rather than asserted in prose: for every pair of
    // (published, held) there is a free slot, so the producer never stalls on the consumer.
    let (_p, console, _producer) = pair("slots", cap(16, 16));
    let h = console.header;
    for latest in 0..h.slots {
        for held in 0..h.slots {
            console
                .map
                .u32_at(h.status_off + crate::wire::status::LATEST_SLOT as u64)
                .store(latest, std::sync::atomic::Ordering::Release);
            console
                .map
                .u32_at(h.control_off + crate::wire::control::READER_HOLD as u64)
                .store(held, std::sync::atomic::Ordering::Release);
            let free = crate::ring::free_slot(&console.map, &h);
            let free = free.unwrap_or_else(|| panic!("no free slot with latest={latest} held={held}"));
            assert_ne!(free, latest);
            assert_ne!(free, held);
        }
    }
}

// ---------------------------------------------------------------------------------------
// A producer that stops — §4.6's fourth row, three ways
// ---------------------------------------------------------------------------------------

#[test]
fn a_producer_that_stops_goes_live_then_stalled_then_lost_and_never_shows_its_last_frame() {
    let (_p, mut console, mut producer) = pair("stop", cap(32, 32));
    let t0 = Instant::now();
    producer.tick();
    producer.draw().unwrap();
    assert!(matches!(console.poll(t0), Poll::Frame(_)));

    producer.stop();

    let live = console.poll(t0 + ms(10));
    assert!(matches!(live, Poll::Holding), "not yet: {live:?}");
    assert_eq!(live.present(), Present::Keep);

    let stalled = console.poll(t0 + ms(60));
    assert!(matches!(stalled, Poll::Stalled { .. }), "expected stalled, got {stalled:?}");
    assert_eq!(stalled.present(), Present::Forget, "hung is in §4.6's fourth row too");

    let lost = console.poll(t0 + ms(150));
    assert!(matches!(lost, Poll::Lost { .. }), "expected lost, got {lost:?}");
    assert_eq!(lost.present(), Present::Forget);

    // 🚨 And the frame is still sitting there, whole and valid, and is still not served.
    for at in [t0 + ms(60), t0 + ms(150), t0 + ms(5000)] {
        assert!(!matches!(console.poll(at), Poll::Frame(_)), "the last good frame was painted");
    }
}

#[test]
fn a_farewell_is_told_apart_from_a_hang() {
    let (_p, mut console, mut producer) = pair("depart", cap(32, 32));
    let t0 = Instant::now();
    producer.tick();
    producer.draw().unwrap();
    assert!(matches!(console.poll(t0), Poll::Frame(_)));

    producer.depart();
    // Immediately, with no silence at all — which is the point: a clean exit does not have to
    // be waited out.
    let gone = console.poll(t0 + ms(1));
    assert!(matches!(gone, Poll::Gone), "expected a farewell, got {gone:?}");
    assert_eq!(gone.present(), Present::Forget);
}

#[test]
fn a_producer_that_never_starts_stops_saying_starting() {
    // §4.6: the third row must time out into the fourth rather than sitting for ever.
    let (_p, mut console, mut producer) = pair("nostart", cap(32, 32));
    let t0 = Instant::now();
    producer.tick();
    assert!(matches!(console.poll(t0 + ms(10)), Poll::Starting { .. }));
    producer.tick();
    let out = console.poll(t0 + ms(500));
    assert!(matches!(out, Poll::Lost { .. }), "expected the deadline to fire, got {out:?}");
}

#[test]
fn a_paused_producer_is_live_for_ever_because_it_still_ticks() {
    // The default lifecycle is Attached, so this is the NORMAL state a module arrives in — a
    // liveness rule that called it dead would make the ordinary case look like the failure.
    let (_p, mut console, mut producer) = pair("paused", cap(32, 32));
    let t0 = Instant::now();
    producer.tick();
    producer.draw().unwrap();
    producer.set_state(ProducerState::Paused);
    assert!(matches!(console.poll(t0), Poll::Frame(_)));

    for step in 1..40 {
        producer.tick();
        let at = t0 + ms(step * 20);
        let out = console.poll(at);
        assert!(matches!(out, Poll::Holding), "at {step}: {out:?}");
        assert_eq!(out.present(), Present::Keep);
    }
}

// ---------------------------------------------------------------------------------------
// Size — the console asks, the producer answers per frame
// ---------------------------------------------------------------------------------------

#[test]
fn a_size_change_lands_without_invalidating_the_frames_already_in_flight() {
    let (_p, mut console, mut producer) = pair("resize", cap(128, 128));
    let now = Instant::now();
    producer.tick();
    producer.draw_at(64, 64).unwrap();
    assert!(matches!(console.poll(now), Poll::Frame(_)));

    assert!(console.ask_size(96, 48));
    assert!(!console.ask_size(96, 48), "asking for what was already asked changes nothing");
    let epoch = console.size_epoch();

    // The producer has not read the request yet and draws one more frame at the old size.
    // 🚨 It is a perfectly good picture and must be served as one.
    producer.tick();
    producer.draw_at(64, 64).unwrap();
    match console.poll(now) {
        Poll::Frame(v) => {
            assert_eq!((v.width, v.height), (64, 64));
            sim::verify(&v).unwrap();
        }
        other => panic!("a frame from before the resize is still a frame: {other:?}"),
    }
    assert_ne!(
        console.status().applied_size_epoch,
        epoch,
        "and the console can tell the resize has not landed"
    );

    producer.adopt_requested_size();
    producer.draw().unwrap();
    match console.poll(now) {
        Poll::Frame(v) => {
            assert_eq!((v.width, v.height), (96, 48));
            assert_eq!(v.size_epoch, epoch, "the frame names the request it was drawn under");
            sim::verify(&v).unwrap();
        }
        other => panic!("expected the resized frame, got {other:?}"),
    }
    assert_eq!(console.status().applied_size_epoch, epoch);
}

#[test]
fn a_request_larger_than_the_channel_is_clamped_rather_than_refused() {
    // A region that grew past what the channel was planned for gets a slightly small picture
    // now, not a black rectangle.
    let (_p, mut console, _producer) = pair("clamp", cap(64, 64));
    console.ask_size(4096, 4096);
    assert_eq!(console.status().drawn, (0, 0));
    assert!(console.capacity().admits(64, 64));
    assert!(!console.capacity().admits(65, 64));
}

#[test]
fn a_frame_outside_the_capacity_cannot_be_begun() {
    let (_p, _console, mut producer) = pair("toobig", cap(32, 32));
    assert!(producer.channel().begin_frame(33, 32).is_none());
    assert!(producer.channel().begin_frame(32, 0).is_none());
    assert!(producer.channel().begin_frame(32, 32).is_some());
}

/// 🚨 T0's condition, as an assertion rather than a comment. A per-frame allocation costs
/// double at 1440p and 3.3× of the gap is page-faulting on bytes that are identical — which
/// reads as a verdict on the mechanism rather than on the allocator.
#[test]
fn nothing_on_the_frame_path_allocates_including_across_a_resize() {
    let (_p, mut console, mut producer) = pair("noalloc", cap(256, 256));
    let now = Instant::now();
    assert_eq!(console.staging_allocations(), 1);
    for step in 0..60 {
        producer.tick();
        // Oscillate the size, which is the case a naive implementation resizes a buffer for.
        let w = 64 + (step % 4) * 32;
        producer.draw_at(w, 64).unwrap();
        console.ask_size(w, 64);
        assert!(matches!(console.poll(now), Poll::Frame(_)));
    }
    assert_eq!(console.staging_allocations(), 1, "the staging buffer is capacity-sized once");
}

// ---------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------

#[test]
fn input_crosses_in_order_and_only_once() {
    let (_p, mut console, mut producer) = pair("input", cap(16, 16));
    let sent = [
        InputEvent::Down(Button::Key(Key::W)),
        InputEvent::Pointer { dx: 3.5, dy: -1.25 },
        InputEvent::Up(Button::Key(Key::W)),
        InputEvent::Down(Button::Mouse(MouseButton::Right)),
    ];
    for ev in sent {
        console.send(ev).unwrap();
    }
    let (got, report) = producer.channel().drain_input();
    assert_eq!(got, &sent[..]);
    assert_eq!(report.events, 4);
    assert!(!report.recovered_from_overflow);
    assert_eq!(report.undecodable, 0);

    let (again, report) = producer.channel().drain_input();
    assert!(again.is_empty());
    assert_eq!(report.events, 0);
}

#[test]
fn the_reserved_key_never_reaches_the_producer() {
    // §5.3, at the one place it can be kept: the console cannot leak the way out by
    // forgetting, because there is no path into the ring that does not ask.
    let (_p, mut console, mut producer) = pair("reserved", cap(16, 16));
    assert_eq!(console.send(InputEvent::Down(Button::Key(Key::Escape))), Err(PushFault::Refused));
    console.send(InputEvent::Down(Button::Key(Key::W))).unwrap();
    let (got, _) = producer.channel().drain_input();
    assert_eq!(got, &[InputEvent::Down(Button::Key(Key::W))]);
}

#[test]
fn an_overflowing_input_ring_recovers_with_a_release_all_rather_than_a_stuck_key() {
    let (_p, mut console, mut producer) = pair("overflow", cap(16, 16));
    let capacity = console.header().input_capacity as usize;

    // A key goes down, then the producer stops reading and the ring fills. The `Up` for that
    // key is among the events that never fit — which is exactly the sequence that leaves a
    // module holding a key for ever if the prefix is replayed.
    console.send(InputEvent::Down(Button::Key(Key::W))).unwrap();
    for _ in 1..capacity {
        console.send(InputEvent::Pointer { dx: 1.0, dy: 0.0 }).unwrap();
    }
    assert_eq!(console.send(InputEvent::Up(Button::Key(Key::W))), Err(PushFault::Full));

    let (got, report) = producer.channel().drain_input();
    assert!(report.recovered_from_overflow);
    assert_eq!(got, &[InputEvent::ReleaseAll], "the buffered prefix is discarded, not replayed");

    // And the ring is usable again immediately.
    console.send(InputEvent::Down(Button::Key(Key::A))).unwrap();
    let (got, report) = producer.channel().drain_input();
    assert!(!report.recovered_from_overflow);
    assert_eq!(got, &[InputEvent::Down(Button::Key(Key::A))]);
}

// ---------------------------------------------------------------------------------------
// Lifecycle and versioning
// ---------------------------------------------------------------------------------------

#[test]
fn a_module_arrives_paused_and_cannot_start_itself() {
    let (_p, mut console, mut producer) = pair("inert", cap(16, 16));
    assert_eq!(console.lifecycle(), Lifecycle::Attached);
    producer.tick();
    assert_eq!(producer.channel().control().lifecycle, Lifecycle::Attached);

    // Everything a producer CAN do, done — and the lifecycle is unmoved, because there is no
    // method on `ProducerChannel` that writes a control word. §3.1's rule, alive.
    producer.draw().unwrap();
    producer.set_state(ProducerState::Producing);
    producer.channel().adopt_size(16, 16);
    producer.tick();
    assert_eq!(producer.channel().control().lifecycle, Lifecycle::Attached);

    console.set_lifecycle(Lifecycle::Running);
    producer.tick();
    assert_eq!(producer.channel().control().lifecycle, Lifecycle::Running);
}

#[test]
fn the_console_can_ask_a_producer_to_go_and_the_producer_can_see_the_console_is_alive() {
    let (_p, mut console, mut producer) = pair("close", cap(16, 16));
    producer.tick();
    assert!(!producer.channel().control().close);
    let seen = producer.channel().console_alive();
    console.heartbeat();
    assert!(producer.channel().console_alive() > seen);

    console.request_close();
    producer.tick();
    assert!(producer.channel().control().close);
}

#[test]
fn a_channel_written_by_a_different_wire_version_is_refused_by_name() {
    let path = TempPath::new("version");
    let console = ModuleChannel::create(&path.0, cap(32, 32)).unwrap();
    drop(console);
    // Rewrite the version word in place, exactly as a differently-built module would have.
    let mut bytes = std::fs::read(&path.0).unwrap();
    crate::wire::put_u32(&mut bytes, crate::wire::OFF_WIRE_VERSION, WIRE_VERSION + 1);
    std::fs::write(&path.0, &bytes).unwrap();

    match ProducerChannel::open(&path.0).err() {
        Some(OpenFault::Wire(WireFault::VersionMismatch { ours, theirs })) => {
            assert_eq!((ours, theirs), (WIRE_VERSION, WIRE_VERSION + 1));
        }
        other => panic!("a mismatch must be a refusal, not a garbled picture: {other:?}"),
    }
    // And the refusal is a sentence a person can act on.
    let line = WireFault::VersionMismatch { ours: 1, theirs: 2 }.sentence();
    assert!(line.contains("rebuild"), "{line}");
}

#[test]
fn opening_a_channel_that_does_not_exist_is_an_io_refusal_not_a_panic() {
    let path = TempPath::new("absent");
    match ProducerChannel::open(&path.0).err() {
        Some(OpenFault::Io(_)) => {}
        other => panic!("expected an io refusal, got {other:?}"),
    }
}

/// The instrument for §4.4's number 2. It is not the number — one process, one clock read —
/// but the subtraction has to work before two processes can be asked to do it.
#[test]
fn a_frame_carries_a_publish_time_the_consumer_can_subtract() {
    let (_p, mut console, mut producer) = pair("age", cap(32, 32));
    producer.tick();
    producer.draw().unwrap();
    match console.poll(Instant::now()) {
        Poll::Frame(v) => {
            let age = v.age().expect("a just-published frame has a believable age");
            assert!(age < Duration::from_secs(1), "{age:?}");
        }
        other => panic!("expected a frame, got {other:?}"),
    }
}
