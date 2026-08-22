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
fn measure(w: u32, h: u32, draw_every: Option<u64>) -> Option<Measured> {
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
    let mut taken = 0usize;
    let mut polls = 0usize;
    // 🚨 **The producer's ACHIEVED period, from its own frame indices** — see `Measured::period`.
    let mut first: Option<(u64, Instant)> = None;
    let mut last: Option<(u64, Instant)> = None;
    // A ceiling so a producer that never draws fails the test rather than hanging it.
    let deadline = Instant::now() + Duration::from_secs(30);
    while taken < SAMPLES + WARMUP && Instant::now() < deadline {
        let next = Instant::now() + POLL;
        polls += 1;
        if let Poll::Frame(view) = console.poll(Instant::now()) {
            // ⚠️ `age` answers `None` on a negative or absurd result — the two clocks are one
            // machine's `SystemTime`, which is shared but not monotonic. A skipped sample is
            // better than a duration nobody can stand behind, and the count is reported.
            if let Some(age) = view.age() {
                taken += 1;
                if taken > WARMUP {
                    ages.push(age.as_secs_f64() * 1000.0);
                    let now = Instant::now();
                    // The frame index counts every frame the producer BEGAN, including ones
                    // this consumer never saw — which is exactly what makes it a measure of the
                    // producer's cadence rather than of the consumer's.
                    first.get_or_insert((view.frame_index, now));
                    last = Some((view.frame_index, now));
                }
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
    if ages.len() < SAMPLES / 2 {
        eprintln!("  {w}x{h}: only {} sample(s) in 30 s — skipped", ages.len());
        return None;
    }
    ages.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(Measured {
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
        let Some(m) = measure(w, h, None) else { continue };
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

/// 🚨 **The control that turns the number above into an explanation.**
///
/// The size sweep shows staleness barely moving between 640×360 and 2560×1440 — nine times the
/// pixels for a few per cent — which is not what a transport cost looks like. The hypothesis is
/// that the age of a frame at poll time is dominated by **the phase between two free-running
/// loops**: the producer publishes every P ms, the consumer looks every 16.7 ms, the two are
/// unsynchronised, so the age at the moment of looking is spread roughly uniformly over P and the
/// median lands near P/2.
///
/// ⚠️ **The two readings have completely different consequences**, which is why this is worth a
/// second test rather than a paragraph. If it is transport, the fix is mechanism A — the shared
/// GPU texture, `unsafe`, per-backend — and §6's *"flying inside a small pane may not be the
/// thing"* is settled against. If it is phase, the transport contributes almost nothing, the fix
/// is a faster producer or a synchronised one, and §6 stays open.
///
/// So: hold the size fixed and move **P**. If the median tracks the period, it is phase.
///
/// # ✏️ The model this test was first written with was wrong, and the measurement said so
///
/// The first version predicted **median ≈ P/2** and asserted a band around it. It passed at 4, 8
/// and 16 ms and failed at 33 ms, reporting 6.43 ms where P/2 is 16.5 — and the failure was the
/// useful part, because the reason is structural rather than noise.
///
/// Once the producer is **slower than the consumer**, the consumer is no longer the thing doing
/// the sampling: every frame is seen at the first poll after it appears, so the age is spread
/// over the *poll* interval instead. The honest model is therefore
///
/// > **median ≈ half of `min(producer period, poll interval)`** — half of whichever loop is
/// > *faster*, because that is the one setting how long a frame sits unlooked-at.
///
/// 🚨 Which strengthens the conclusion rather than weakening it: staleness is bounded by the two
/// **cadences** at both ends of the sweep, and the frame size — nine times the pixels between the
/// smallest and largest — moves it by a few per cent. The transport is not what a person would be
/// waiting for.
#[test]
#[ignore = "spawns a producer process and measures wall-clock; run explicitly"]
fn staleness_tracks_the_producers_period_rather_than_the_frame_size() {
    println!("\nstaleness against the PRODUCER's period, fixed at 1280x720, consumer at 60 Hz\n");
    let poll_ms = POLL.as_secs_f64() * 1000.0;
    println!(
        "  {:>8}  {:>9}  {:>9}  {:>9}  {:>11}  {:>13}",
        "asked", "achieved", "median", "p90", "min(P,16.7)", "median / that"
    );
    let mut rows = Vec::new();
    for p in [4u64, 8, 16, 33] {
        let Some(m) = measure(1280, 720, Some(p)) else { continue };
        // 🚨 **The ACHIEVED period, never the asked-for one.** See `Measured::period`: under an
        // unoptimised build the simulator cannot honour a 4 ms request, every condition collapses
        // to the same real cadence, and a rig reading the flag would conclude the transport is at
        // fault. Reading the frame indices makes that run a *confirming* point instead.
        let sampling = m.period.min(poll_ms);
        let ratio = m.median / sampling;
        rows.push((p, m.period, m.median, sampling, ratio));
        println!(
            "  {p:>5} ms  {:>6.1} ms  {:>6.2} ms  {:>6.2} ms  {sampling:>8.1} ms  {ratio:>12.2}",
            m.period, m.median, m.p90
        );
    }
    assert!(rows.len() >= 3, "not enough cadences produced a measurement");

    // 🚨 **The claim: the median is a fraction of the sampling window, whatever that window is.**
    // If staleness were the transport it would be a *constant* instead — flat in milliseconds
    // while the window moved — and this band is what fails then. That is the reading that would
    // reopen §4.4's mechanism-A question, so it gets an assertion rather than a paragraph.
    //
    // ⚠️ A wide band on purpose. This is a claim about a *mechanism*; pinning it to a tight
    // constant would make it a flaky test about this machine's scheduler. 0.2–1.0 fails a flat
    // line and fails a median larger than the whole window (which would mean frames are being
    // missed rather than sampled), and those are the two readings that change what gets built.
    for (asked, achieved, med, sampling, ratio) in &rows {
        assert!(
            (0.2..=1.0).contains(ratio),
            "asked for {asked} ms, the producer achieved {achieved:.1} ms, and the median age was \
             {med:.2} ms — {ratio:.2} of the {sampling:.1} ms sampling window, where the phase \
             model predicts about half. A ratio far below the band means staleness is NOT tracking \
             the window, i.e. it is the TRANSPORT, and §4.4's mechanism-A question is reopened"
        );
    }
    // And the sweep has to have actually swept — otherwise every row above is one condition
    // measured four times, which would satisfy the band while proving nothing.
    let widest = rows.iter().map(|r| r.3).fold(f64::MIN, f64::max);
    let narrowest = rows.iter().map(|r| r.3).fold(f64::MAX, f64::min);
    assert!(
        widest > narrowest * 1.8,
        "every condition ended up sampling over about the same window ({narrowest:.1}–\
         {widest:.1} ms), so this run says nothing about whether staleness tracks it. The usual \
         cause is an unoptimised producer that cannot honour the fast cadences — build with \
         --release."
    );
}
