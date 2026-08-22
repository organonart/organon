//! **§4.4's number 2, taken rather than reasoned about** — how stale the frame a consumer takes
//! actually is, across two processes.
//!
//! `doc/organon_module_viewport.md` §4.4 asked for three numbers and got two from T0
//! (`doc/measurements/module-frame-boundary-2026-08-21.md`): the producer's added stall, and the
//! full round trip. Both are **throughput**. The third is **staleness**, and the design refused
//! to let anybody estimate it:
//!
//! > Frames of latency between *"the module drew it"* and *"the console painted it"* was not
//! > attempted, because it needs a second process and a protocol that does not exist.
//!
//! Both now exist. `organon-module-sim` is a producer in its own process; every frame carries the
//! producer's `SystemTime` at publish; and [`FrameView::age`] is the subtraction. This is the rig
//! that performs it.
//!
//! ```text
//! cargo test -p organon-module --features sim --release --test staleness \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! 🚨 **`--test-threads=1` is part of the command, not tidiness.** Both tests here launch a
//! producer and time it; run in parallel they share a CPU and each reports the other's load as
//! its own latency. The channel-file collision that *also* came from running them together is
//! fixed in [`measure`] — but a unique filename only removes the corruption, not the
//! interference, and a timing rig that is quietly measuring a second copy of itself is the kind
//! of wrong that looks entirely reasonable on the page.
//!
//! # 🚨 What this measures, and the two things it deliberately does not
//!
//! It measures **publish → the consumer holds the pixels**: the producer's `commit`, the console's
//! next poll, the seqlock read and the memcpy into staging. That is the part the *protocol* owns,
//! and it is the part that would have to change if the answer were bad.
//!
//! It does **not** include:
//!
//! 1. **The console's own frame** — egui's layout and paint, `write_texture`, the render pass and
//!    the swapchain present, which happen after the poll and are the same cost the console
//!    already pays for every other picture in the window.
//! 2. **The producer's render**, which is Ascent's `render`/`read_frame` and was measured on that
//!    side (#84: render median 0.12 ms, readback median 0.61 ms at 1080p).
//!
//! ⚠️ So the honest headline is *"the transport adds N ms of age to a frame"*, and a number for
//! *"what is on the glass is N ms old"* would be this plus a console frame. Reporting the second
//! from this rig would be quoting a measurement for something it did not measure — the failure
//! `doc/measurements/` already carries a rule about.
//!
//! # ⚠️ Why the cadence is the console's, not as fast as possible
//!
//! The consumer polls at **60 Hz**, because staleness is dominated by *when you look*, not by how
//! long the copy takes. A rig that polled in a tight loop would measure the copy and report a
//! number three times better than any console will ever see — which is exactly the shape of
//! wrongness §4.4 warns about in the other direction.

#![cfg(feature = "sim")]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use organon_module::{
    FrameCapacity, ModuleChannel, PixelFormat, Poll, CHANNEL_ENV,
};

/// The console's cadence. 60 Hz is what this machine's console runs at.
const POLL: Duration = Duration::from_micros(16_667);

/// How many frames each size is measured over, after warm-up.
const SAMPLES: usize = 240;

/// Frames discarded first — the producer's first loop allocates, and the page cache is cold.
const WARMUP: usize = 60;

/// A child that is killed even if an assertion unwinds past it.
struct Producer(Child);

impl Drop for Producer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

/// One size: launch a producer, poll at 60 Hz, and report the age of every frame taken.
fn measure(w: u32, h: u32, draw_every: Option<u64>, want: usize) -> Option<Measured> {
    // ⚠️ **A counter, not just the pid and the size** — and it was a real defect, not a
    // precaution. The two tests here run on different threads of one process and both measure
    // 1280×720, so a name built from `(pid, w, h)` gave them **one channel file**: two consoles
    // and two producers on one mapping. The symptom was not an error — one test reported
    // `only 0 sample(s)` while the other reported a median 30 % better than its own solo run,
    // which reads as a plausible measurement rather than as a collision.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "organon-module-staleness-{}-{n}-{w}x{h}.frames",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut console =
        ModuleChannel::create(&path, FrameCapacity::new(w, h, PixelFormat::Rgba8UnormSrgb))
            .expect("the channel could not be created");
    console.ask_size(w, h);

    let child = Command::new(env!("CARGO_BIN_EXE_organon-module-sim"))
        .env(CHANNEL_ENV, &path)
        .args(draw_every.map_or(vec![], |ms| vec!["--draw-every-ms".to_string(), ms.to_string()]))
        .spawn()
        .expect("organon-module-sim would not start");
    let _producer = Producer(child);

    let mut ages: Vec<f64> = Vec::with_capacity(SAMPLES);
    // 🚨 **The second quantity, and it is the one a person experiences.** See `Measured::onscreen`.
    let mut onscreen: Vec<f64> = Vec::with_capacity(SAMPLES * 4);
    let mut held: Option<(f64, Instant)> = None;
    let mut taken = 0usize;
    let mut polls = 0usize;
    // 🚨 **The producer's ACHIEVED period, from its own frame indices** — see `Measured::period`.
    let mut first: Option<(u64, Instant)> = None;
    let mut last: Option<(u64, Instant)> = None;
    // A ceiling so a producer that never draws fails the test rather than hanging it.
    // ⚠️ Scaled to the condition: a producer drawing every 250 ms needs 60 frames and 15 s, and a
    // fixed 30 s ceiling would silently skip exactly the slow-producer rows this rig now exists
    // to measure — the regime where the two candidate laws disagree.
    let deadline = Instant::now()
        + Duration::from_millis(20_000 + (want as u64 + WARMUP as u64) * draw_every.unwrap_or(17));
    while taken < want + WARMUP && Instant::now() < deadline {
        let next = Instant::now() + POLL;
        polls += 1;
        // 🚨 **The rig has to behave like a console, and this line was missing.** A real host calls
        // `heartbeat()` once per frame (`ModuleHost::observe` does); `poll()` does not do it for
        // you. Without it the producer's own symmetric judgement fires — `organon-module-sim`
        // leaves after twenty seconds of a motionless console counter, exactly as it should — and
        // the run simply ends.
        //
        // ⚠️ **The symptom was a skipped row, not an error**, and it skipped precisely the slowest
        // condition, because that is the one whose run exceeds twenty seconds. So the rig was
        // quietly unable to measure the regime it was extended to measure, and the P = 100 row
        // was finishing inside the window by luck. Found by asking why 250 ms produced 24 samples
        // when the arithmetic said 140 would fit.
        console.heartbeat();
        if let Poll::Frame(view) = console.poll(Instant::now()) {
            // ⚠️ `age` answers `None` on a negative or absurd result — the two clocks are one
            // machine's `SystemTime`, which is shared but not monotonic. A skipped sample is
            // better than a duration nobody can stand behind, and the count is reported.
            if let Some(age) = view.age() {
                taken += 1;
                let now = Instant::now();
                // What the console now holds, and when it started holding it. Kept whether or
                // not this sample counts, so the on-screen figure is never computed from a
                // frame acquired during warm-up.
                held = Some((age.as_secs_f64() * 1000.0, now));
                if taken > WARMUP {
                    ages.push(age.as_secs_f64() * 1000.0);
                    // The frame index counts every frame the producer BEGAN, including ones
                    // this consumer never saw — which is exactly what makes it a measure of the
                    // producer's cadence rather than of the consumer's.
                    first.get_or_insert((view.frame_index, now));
                    last = Some((view.frame_index, now));
                }
            }
        }
        // 🚨 **Sampled at EVERY poll, including the ones that returned nothing.** That is the
        // whole difference between the two figures: `ages` only hears from polls that took a
        // frame, and the console goes on painting the one it holds through every poll in
        // between. A frame the console keeps for 100 ms is on the glass for 100 ms.
        if taken > WARMUP {
            if let Some((at_acquire, when)) = held {
                onscreen.push(at_acquire + when.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
    }
    let period = match (first, last) {
        (Some((i0, t0)), Some((i1, t1))) if i1 > i0 => {
            (t1 - t0).as_secs_f64() * 1000.0 / (i1 - i0) as f64
        }
        _ => f64::NAN,
    };
    console.request_close();
    let _ = std::fs::remove_file(&path);
    // ⚠️ Against `want`, not the module-wide `SAMPLES`. A slow-producer condition deliberately
    // asks for fewer frames — 80 at a 250 ms cadence is already 20 s — and a fixed threshold of
    // `SAMPLES / 2` silently discarded exactly the rows this rig exists to measure, printing
    // "only 80 sample(s)" for a condition that had collected every one it asked for.
    if ages.len() < want / 2 {
        eprintln!(
            "  {w}x{h}: only {} of {want} sample(s) before the deadline — skipped",
            ages.len()
        );
        return None;
    }
    ages.sort_by(|a, b| a.partial_cmp(b).unwrap());
    onscreen.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(Measured {
        onscreen: percentile(&onscreen, 0.5),
        median: percentile(&ages, 0.5),
        p90: percentile(&ages, 0.9),
        worst: *ages.last().unwrap(),
        period,
        polls,
        torn: console.torn_reads(),
    })
}

/// One condition's result.
struct Measured {
    /// 🚨 **The age of the picture ON THE GLASS, sampled at every poll** — the quantity a person
    /// experiences, and **not** the one [`Measured::median`] reports.
    ///
    /// ⚠️ **Two different questions, and this rig originally answered only the first.** `median`
    /// is the age of a frame *at the moment the console takes it*, sampled only on polls that
    /// returned one. Between takes the console goes on painting what it holds, and that frame
    /// keeps ageing — so a producer drawing every 100 ms leaves a picture up for 100 ms however
    /// often the console looks.
    ///
    /// 📌 **The distinction is invisible whenever the producer is at least as fast as the poll**,
    /// because then every poll takes a new frame and the two coincide. Every condition measured
    /// before this field existed was ~60 Hz on both sides, so the terms never separated — which
    /// is how a relation can fit the data perfectly and still be the wrong law.
    onscreen: f64,
    median: f64,
    p90: f64,
    worst: f64,
    /// 🚨 **The period the producer ACHIEVED, not the one it was asked for** — and the
    /// difference between those two is a trap this rig fell into and now cannot fall into again.
    ///
    /// ⚠️ **The failure it prevents, because it produced a confident wrong verdict.** Run under
    /// `CARGO_PROFILE_TEST_OPT_LEVEL=0` — which is *this repository's standard bar setting*, so
    /// somebody will — the simulator's per-pixel draw loop is unoptimised and 1280×720 takes
    /// ~15 ms whatever `--draw-every-ms` says. Every requested cadence then collapses to the same
    /// real one, every median comes out flat at ~8.3 ms, and the control test concluded *"the
    /// median did not move with the producer's period — staleness is the TRANSPORT"*. That is the
    /// verdict that reopens §4.4's mechanism-A question, reached because the lever was not
    /// connected to anything.
    ///
    /// 📌 Reading it off the **frame indices** rather than off the flag makes the model hold in
    /// both builds: the debug run stops being a contradiction and becomes another point on the
    /// same line, at a period nobody asked for. That is strictly better than refusing to run
    /// unoptimised, because a rig that only works in one configuration is a rig whose one
    /// configuration eventually stops being used.
    period: f64,
    polls: usize,
    torn: u64,
}

/// 🚨 **The measurement §4.4 has been missing since it was written.**
///
/// `#[ignore]`d on `frame_boundary.rs`'s precedent: it launches a process, sleeps for seconds and
/// reports numbers, none of which belongs in a suite that has to stay fast and has to pass on a
/// machine with nothing to measure.
#[test]
#[ignore = "spawns a producer process and measures wall-clock; run explicitly"]
fn how_stale_is_the_frame_the_console_takes() {
    println!("\nstaleness — publish to consumer-holds-the-pixels, consumer polling at 60 Hz");
    println!("  {SAMPLES} frames per size after {WARMUP} warm-up\n");
    println!("  {:>11}  {:>9}  {:>9}  {:>9}  {:>6}", "size", "median", "p90", "worst", "torn");
    let mut any = false;
    for (w, h) in [(640u32, 360u32), (1280, 720), (1920, 1080), (2560, 1440)] {
        let Some(m) = measure(w, h, None, SAMPLES) else { continue };
        let (med, p90, worst, polls, torn) = (m.median, m.p90, m.worst, m.polls, m.torn);
        any = true;
        println!(
            "  {:>11}  {med:>6.2} ms  {p90:>6.2} ms  {worst:>6.2} ms  {torn:>6}   ({polls} polls, \
             producer drew every {:.1} ms, median = {:.2} frames at 60 Hz)",
            format!("{w}x{h}"),
            m.period,
            med / 16.667,
        );
        // 🚨 The claim, not just the print. A frame the console takes must be **younger than one
        // console frame** — that is what "the copy is affordable" has to mean at the point of
        // use, and it is the threshold §6 says stops being affordable at full screen.
        assert!(
            med < 16.667,
            "at {w}x{h} the median frame is {med:.2} ms old — older than a 60 Hz frame, which \
             makes the picture at least one frame behind before the console has drawn anything"
        );
        assert_eq!(torn, 0, "the simulator ignored the reader's hold at {w}x{h}");
    }
    assert!(any, "no size produced a measurement — the producer never drew");
}

/// 🚨 **The control that turns the number above into an explanation — and the experiment that
/// separates two laws which agreed on every condition measured before it.**
///
/// The size sweep shows staleness barely moving between 640×360 and 2560×1440 — nine times the
/// pixels for a few per cent — which is not what a transport cost looks like. So the hypothesis
/// is **phase**: two free-running loops sampling each other. Hold the size fixed, move the
/// producer's period P, and watch.
///
/// ⚠️ **The two readings have completely different consequences.** If it is transport, the fix is
/// mechanism A — the shared GPU texture, `unsafe`, per-backend — and §6's *"flying inside a small
/// pane may not be the thing"* is settled against. If it is phase, the transport contributes
/// almost nothing and §6 stays open.
///
/// # 🚨 Two quantities, and the difference is invisible at 60 Hz on both sides
///
/// This rig reports **two** medians, because there are two questions and they are not the same:
///
/// | | what it is | sampled |
/// |---|---|---|
/// | **acquired** | how old a frame is **when the console takes it** | only on polls that returned one |
/// | **on screen** | how old the picture **currently being painted** is | **every** poll |
///
/// They coincide whenever the producer is at least as fast as the poll, because then every poll
/// takes a fresh frame — and every condition measured before this test existed was ~60 Hz on both
/// sides. 📌 **That is how a relation can fit all the data and still be the wrong law.** The
/// published `0.55 × min(P, Q)` describes *acquired*; a person looking at a rectangle is
/// experiencing *on screen*, and once P > Q the console holds one frame across many polls while
/// it ages.
///
/// ✏️ **The history is worth keeping, because the same mistake happened twice.** A first version
/// predicted `median ≈ P/2` for the acquired figure and failed at a 33 ms period; the fix — half
/// of whichever loop is *faster* — was right about acquisition and was then published as though
/// it described what a person sees. The Ascent session caught it by noticing that a table in one
/// of my own messages computed `0.55 × P` while the formula printed beside it said
/// `0.55 × min(P, Q)`. **The arithmetic was producer-dominated; only the formula was not.**
#[test]
#[ignore = "spawns a producer process and measures wall-clock; run explicitly"]
fn what_is_on_screen_tracks_the_producer_and_what_is_acquired_tracks_the_faster_loop() {
    let q = POLL.as_secs_f64() * 1000.0;
    println!("\nstaleness, 1280x720, consumer polling every {q:.1} ms\n");
    println!(
        "  {:>7} {:>9} {:>10} {:>10} {:>8} {:>9} {:>9} {:>9}",
        "asked", "achieved", "acquired", "on screen", "0.5P", "0.5min", "acq/min", "scr/P"
    );
    let mut rows = Vec::new();
    // 🚨 P = 100 and P = 250 are the point of this list. `min(P, Q)` predicts ~8 ms for both;
    // `0.5 × P` predicts 50 ms and 125 ms. A 6× and a 15× gap — one run decides it.
    for p in [8u64, 16, 33, 100, 250] {
        // Fewer samples as the producer slows, or a 250 ms cadence would need a minute.
        let want = if p >= 100 { 80 } else { 200 };
        let Some(m) = measure(1280, 720, Some(p), want) else { continue };
        let half_p = 0.5 * m.period;
        let half_min = 0.5 * m.period.min(q);
        rows.push((p, m.period, m.median, m.onscreen, half_p, half_min));
        println!(
            "  {p:>4} ms {:>6.1} ms {:>7.2} ms {:>7.2} ms {half_p:>5.1} ms {half_min:>6.1} ms \
             {:>8.2} {:>9.2}",
            m.period,
            m.median,
            m.onscreen,
            m.median / half_min,
            m.onscreen / m.period,
        );
    }
    assert!(rows.len() >= 4, "not enough cadences produced a measurement");

    let separated: Vec<_> = rows.iter().filter(|r| r.1 > q * 2.0).collect();
    assert!(
        !separated.is_empty(),
        "no condition had a producer meaningfully slower than the poll, so this run cannot \
         distinguish the two laws — which is exactly the hole it exists to close. The usual \
         cause is an unoptimised producer; build with --release."
    );

    for (asked, period, acquired, onscreen, half_p, half_min) in &separated {
        // 🚨 **The claim about what a person sees: it tracks the PRODUCER, and the poll interval
        // does not enter.** This is the assertion that fails if `min` were the law for the
        // on-screen figure — at P = 100 that would be ~8 ms against a measured ~50.
        let ratio = onscreen / period;
        assert!(
            (0.35..=0.85).contains(&ratio),
            "asked {asked} ms, achieved {period:.1} ms: the picture on screen was {onscreen:.1} \
             ms old, {ratio:.2} of the producer's period. The producer-dominated law predicts \
             about half ({half_p:.1} ms); `min(P, Q)` would predict {half_min:.1} ms"
        );
        // …and the acquired figure does NOT track the producer once it is slower than the poll,
        // because the console takes each frame at the first poll after it appears.
        assert!(
            acquired < &(period * 0.35),
            "asked {asked} ms: the ACQUIRED age was {acquired:.1} ms, which is tracking the \
             producer rather than the poll — the two figures were supposed to separate here"
        );
    }

    // And the acquired figure follows the faster loop across the whole sweep, which is the
    // relation that was published and is correct about the quantity it names.
    for (asked, _, acquired, _, _, half_min) in &rows {
        let ratio = acquired / half_min;
        assert!(
            (0.4..=2.0).contains(&ratio),
            "asked {asked} ms: acquired age {acquired:.2} ms is {ratio:.2}x half the faster \
             loop's period ({half_min:.1} ms), which is not the relation `min` describes"
        );
    }
}
