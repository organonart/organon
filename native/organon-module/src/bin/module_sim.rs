//! **A producer in its own process** — the second half of the protocol, as a program.
//!
//! 🚨 **This exists because two processes are the only way to measure the number `lib.rs` says
//! this crate does not have.** §4.4's number 2 — frames of latency between *"the module drew
//! it"* and *"the console painted it"* — is a subtraction over
//! [`organon_module::FrameView::age`], and both halves of the test suite live in one process,
//! which proves correctness and says nothing about staleness. A binary makes it two.
//!
//! ⚠️ **It is not a stand-in for a module and must never become one.** It draws
//! [`organon_module::sim::pixel`]'s frame-keyed gradient — every pixel a function of
//! `(x, y, frame_index)` — which is a picture chosen so that a *tear* is visible and a *stride
//! bug* is visible, not one chosen to look like anything. A real module is
//! `organonart/ascent`; this is the instrument you point the console at when you need to know
//! whether the console is right, with no second repository in the answer.
//!
//! It is behind the `sim` feature for the reason the feature exists: [`SimProducer`] can write
//! a slot while ignoring the reader's hold, and that is a capability no shipped build should
//! carry.
//!
//! # What it honours, and why each one is not optional
//!
//! * **[`organon_module::CHANNEL_ENV`]** — the whole handoff. A missing variable is a refusal
//!   naming the variable, never a default path, because a producer that guessed would open
//!   *somebody else's* channel.
//! * **[`organon_module::TICK_FLOOR`]** — it ticks at 4 Hz *while paused*, which is the state
//!   every module arrives in. A producer that stopped looping when it had nothing to draw would
//!   read as hung in the normal case.
//! * **The size the console asks for**, adopted through
//!   [`organon_module::ProducerChannel::adopt_size`] before the frame that uses it.
//! * **`close`** — it says goodbye and exits, so the console gets `Gone` rather than a silence
//!   it has to time out.
//!
//! # The levers, for driving the console's failure sentences from outside
//!
//! | Flag | What the console must then say |
//! |---|---|
//! | `--refuse-after N` | *has stopped drawing* — alive, ticking, publishing nothing |
//! | `--stop-after N` | *has stopped responding* → *stopped producing frames* — the loop ends, no farewell |
//! | `--depart-after N` | *has exited* — the farewell path |
//! | `--exit-after N` | the process **ends**, which is the one verdict only a launcher's process handle can reach |
//! | `--never-draw` | *is starting* → and, after `start_within`, *stopped producing frames* |
//!
//! 📌 Every one of those is a state the console has to name, and before this binary existed
//! each was reachable only from inside a unit test. A sentence nobody has seen rendered is a
//! sentence nobody has checked the width of.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use organon_module::sim::SimProducer;
use organon_module::{Lifecycle, ProducerChannel, RefusalReason, CHANNEL_ENV};

/// The paused cadence. Comfortably inside [`organon_module::TICK_FLOOR`], so the producer is
/// never accused of having stopped while it is merely doing what it was told.
const PAUSED_TICK: Duration = Duration::from_millis(100);

/// The running cadence. Not a frame rate the protocol asks for — a producer draws when it
/// draws — but a sim that spun as fast as it could would spend the machine on a gradient.
const DRAW_TICK: Duration = Duration::from_millis(16);

struct Args {
    channel: PathBuf,
    refuse_after: Option<u64>,
    stop_after: Option<u64>,
    depart_after: Option<u64>,
    exit_after: Option<u64>,
    never_draw: bool,
    /// Draw even while `Attached`. **The default**, and it is the contract's: §5.1's
    /// `Attached` is *"the producer exists and is drawing, but time is not advancing"*, so a
    /// paused module still has a picture. `--no-attached-frames` is the lever for checking what
    /// the console says when it has none.
    attached_frames: bool,
    /// How long to sleep between frames, in milliseconds.
    ///
    /// 🚨 **The lever that turns the staleness measurement from a number into an explanation.**
    /// Two free-running loops sample each other, so the age of a frame at poll time is spread
    /// over the *producer's* period — which means the median is about half that period and has
    /// almost nothing to do with the size of the copy. Changing this and watching the median
    /// track it is what distinguishes "the transport is slow" from "you looked at the wrong
    /// moment", and those two have completely different fixes.
    draw_every_ms: u64,
}

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(why) => {
            eprintln!("organon-module-sim: {why}");
            std::process::exit(2);
        }
    };
    let channel = match ProducerChannel::open(&args.channel) {
        Ok(c) => c,
        Err(fault) => {
            eprintln!("organon-module-sim: {}", fault.sentence());
            std::process::exit(1);
        }
    };
    let cap = channel.capacity();
    eprintln!(
        "organon-module-sim: attached to {} — capacity {}x{} {:?}",
        args.channel.display(),
        cap.max_width,
        cap.max_height,
        cap.format
    );
    let mut sim = SimProducer::new(channel);
    let started = Instant::now();
    let mut loops = 0u64;
    let mut drawn = 0u64;
    let mut epoch = u64::MAX;

    loop {
        loops += 1;
        sim.tick();
        let control = sim.channel().control();

        if control.close {
            eprintln!("organon-module-sim: the console asked me to close — {drawn} frame(s)");
            sim.depart();
            return;
        }
        // ⚠️ Checked against the LOOP count, not the frame count: `--stop-after` has to be
        // reachable for a producer that is refusing, which by definition draws nothing.
        if args.exit_after == Some(loops) {
            eprintln!("organon-module-sim: exiting without a farewell at loop {loops}");
            // No `depart`. This is the crash shape — the console's only witness is its own
            // process handle, which is precisely the thing being exercised.
            std::process::exit(3);
        }
        if args.depart_after == Some(loops) {
            eprintln!("organon-module-sim: saying goodbye at loop {loops}");
            sim.depart();
            return;
        }
        if args.stop_after == Some(loops) {
            eprintln!("organon-module-sim: going quiet at loop {loops} — no farewell");
            sim.stop();
        }
        if args.refuse_after == Some(loops) {
            eprintln!("organon-module-sim: refusing at loop {loops}");
            sim.refuse(Some(RefusalReason::SizeUnsupported));
        }

        // The console's request is level-triggered: read it when ready, act once.
        if control.size_epoch != epoch {
            epoch = control.size_epoch;
            sim.adopt_requested_size();
            let (w, h) = control.requested;
            eprintln!("organon-module-sim: drawing at {w}x{h} (epoch {epoch})");
        }

        let should_draw = !args.never_draw
            && (args.attached_frames || control.lifecycle == Lifecycle::Running);
        if should_draw && sim.draw().is_some() {
            drawn += 1;
        }

        // 🚨 A slow loop while paused, never a stopped one. See this file's header.
        let cadence = if control.lifecycle == Lifecycle::Running || args.attached_frames {
            Duration::from_millis(args.draw_every_ms)
        } else {
            PAUSED_TICK
        };
        std::thread::sleep(cadence);

        // A producer whose console has gone away is an orphan holding a GPU. Twenty seconds of
        // a motionless console counter is the symmetric half of the judgement the console makes
        // about this process.
        if started.elapsed() > Duration::from_secs(1) && stale_console(&mut sim) {
            eprintln!("organon-module-sim: nobody is watching — leaving");
            sim.depart();
            return;
        }
    }
}

/// Has the console's own liveness counter stood still for twenty seconds?
fn stale_console(sim: &mut SimProducer) -> bool {
    use std::cell::Cell;
    thread_local! {
        static LAST: Cell<(u64, Option<Instant>)> = const { Cell::new((0, None)) };
    }
    let now = sim.channel().console_alive();
    LAST.with(|l| {
        let (seen, at) = l.get();
        match at {
            Some(at) if seen == now => at.elapsed() > Duration::from_secs(20),
            _ => {
                l.set((now, Some(Instant::now())));
                false
            }
        }
    })
}

fn parse() -> Result<Args, String> {
    let channel = std::env::var(CHANNEL_ENV).map_err(|_| {
        format!(
            "${CHANNEL_ENV} is not set — a hosted producer is told where its channel is and \
             never guesses, because a guessed path is another module's channel"
        )
    })?;
    let mut args = Args {
        channel: PathBuf::from(channel),
        refuse_after: None,
        stop_after: None,
        depart_after: None,
        exit_after: None,
        never_draw: false,
        attached_frames: true,
        draw_every_ms: DRAW_TICK.as_millis() as u64,
    };
    let mut rest = std::env::args().skip(1);
    while let Some(word) = rest.next() {
        let mut count = |what: &str| -> Result<u64, String> {
            rest.next()
                .ok_or_else(|| format!("{what} wants a loop count"))?
                .parse::<u64>()
                .map_err(|e| format!("{what}: {e}"))
        };
        match word.as_str() {
            "--refuse-after" => args.refuse_after = Some(count("--refuse-after")?),
            "--stop-after" => args.stop_after = Some(count("--stop-after")?),
            "--depart-after" => args.depart_after = Some(count("--depart-after")?),
            "--exit-after" => args.exit_after = Some(count("--exit-after")?),
            "--never-draw" => args.never_draw = true,
            "--no-attached-frames" => args.attached_frames = false,
            "--draw-every-ms" => args.draw_every_ms = count("--draw-every-ms")?,
            other => return Err(format!("`{other}` is not a flag this producer takes")),
        }
    }
    Ok(args)
}
