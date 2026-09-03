//! `organon-glyphs` — run a `ttfx` text effect headless and publish its cell grid into
//! the glyph ring (organon#217 T1, `doc/pbr_text_engine.md` §6.1 / §8).
//!
//! The loop is `motion → settle → dwell → next`: tick the effect at `--tick-hz`,
//! publish every frame; when `next_frame` returns `None` hold the settled grid on the
//! ring for `--dwell` seconds (republished as a heartbeat, so a world that opens the
//! ring mid-dwell sees it — and so `generation` stays put, because a heartbeat is not
//! a change); then pick the next effect. §8 measured that every effect settles and
//! almost none hold, so the hold is ours, and this is where it lives.
//!
//! The binary has no window, no PTY and no terminal — `terminal_config` tells ttfx to
//! ignore the one it may or may not be attached to. Kill it and the ring simply stops
//! advancing; the world notices nothing but silence.

use clap::Parser;
use organon_core::glyph_ring::{
    set_frame_name, GlyphFrame, GlyphRingWriter, FRAME_SETTLED, TTFX_CELL_ASPECT,
};
use organon_glyphs::{effect_names, pick_next, Persistence, Producer};
use std::time::{Duration, Instant};
use ttfx::utils::rng::Rng;

#[derive(Parser, Debug)]
#[command(name = "organon-glyphs", about = "Publish a ttfx text effect into Organon's glyph ring", version)]
struct Args {
    /// Text file to animate. Omit to read stdin.
    #[arg(short = 'i', long = "input")]
    input: Option<std::path::PathBuf>,
    /// Effect name (`organon-glyphs --list`). Omit for a random effect per cycle.
    #[arg(short = 'e', long = "effect")]
    effect: Option<String>,
    /// Only these effects in the random rotation.
    #[arg(long = "include", num_args = 1..)]
    include: Vec<String>,
    /// Never these effects in the random rotation.
    #[arg(long = "exclude", num_args = 1..)]
    exclude: Vec<String>,
    /// The effect's own frame rate — what its timing was authored against (Omarchy
    /// passes 120). Changes how many ticks an effect takes, not how fast we go.
    #[arg(long = "fps", default_value_t = 120)]
    fps: i64,
    /// How many ticks per second we publish. Defaults to `--fps` (real time); lower
    /// plays the effect in slow motion, which the world's interpolation then smooths.
    #[arg(long = "tick-hz")]
    tick_hz: Option<f64>,
    /// Seconds to hold the settled text before the next effect (§8's dwell).
    #[arg(long = "dwell", default_value_t = 4.0)]
    dwell: f64,
    /// Heartbeat interval during the dwell, in milliseconds.
    #[arg(long = "heartbeat-ms", default_value_t = 250)]
    heartbeat_ms: u64,
    /// Phosphor persistence: the time constant, in milliseconds, over which a cell's
    /// emission decays after its source goes dark or dims (§15 T11). A trail keeps its
    /// symbol and is flagged `PERSIST` in the ring. 0 = off, and off is byte-identical
    /// to a producer without it.
    #[arg(long = "persist-ms", default_value_t = 0.0)]
    persist_ms: f64,
    /// Seed for effect choice and every effect's own randomness. Omit for entropy.
    #[arg(long = "seed")]
    seed: Option<u64>,
    /// Force the canvas width (cells). Default: the input's widest line.
    #[arg(long = "cols")]
    cols: Option<i64>,
    /// Force the canvas height (cells). Default: the input's line count.
    #[arg(long = "rows")]
    rows: Option<i64>,
    /// Run one effect (and its dwell) and exit.
    #[arg(long = "once", default_value_t = false)]
    once: bool,
    /// Run as fast as possible with no dwell — for measuring, never for viewing.
    #[arg(long = "no-pace", default_value_t = false, hide = true)]
    no_pace: bool,
    /// Print the effect registry and exit.
    #[arg(long = "list", default_value_t = false)]
    list: bool,
}

fn read_input(path: Option<&std::path::Path>) -> Result<String, String> {
    match path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("reading {}: {e}", p.display())),
        None => {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| format!("reading stdin: {e}"))?;
            Ok(s)
        }
    }
}

fn main() {
    let args = Args::parse();
    if args.list {
        for n in effect_names() {
            println!("{n}");
        }
        return;
    }
    if let Err(e) = run(&args) {
        eprintln!("organon-glyphs: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let input = read_input(args.input.as_deref())?;
    if input.trim().is_empty() {
        return Err("no input text".into());
    }
    let tick_hz = args.tick_hz.unwrap_or(args.fps as f64).max(0.01);
    let period = Duration::from_secs_f64(1.0 / tick_hz);
    let seed = args.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
    });
    let mut picker = Rng::seeded(seed);
    let mut names = effect_names();
    if !args.include.is_empty() {
        names.retain(|n| args.include.contains(n));
    }
    names.retain(|n| !args.exclude.contains(n));
    if args.effect.is_none() && names.is_empty() {
        return Err("no effects left after --include/--exclude".into());
    }

    let mut writer = GlyphRingWriter::create(TTFX_CELL_ASPECT, tick_hz as f32)
        .map_err(|e| format!("creating the ring: {e}"))?;
    eprintln!(
        "organon-glyphs: ring at {} ({} Hz, seed {seed})",
        organon_core::ipc::glyph_ring_path().display(),
        tick_hz
    );

    let mut cells = Vec::new();
    let mut previous: Option<String> = None;
    let mut epoch: u32 = 0;
    // T11: one set of phosphors for the whole run, so the settled text of one effect
    // fades under the opening of the next. Off (the default) touches nothing.
    let mut persist = Persistence::new(args.persist_ms);
    if persist.enabled() {
        eprintln!("organon-glyphs: persistence τ = {} ms", args.persist_ms);
    }
    let beat = Duration::from_millis(args.heartbeat_ms.max(10));
    loop {
        let name = match &args.effect {
            Some(n) => n.clone(),
            None => pick_next(&mut picker, &names, previous.as_deref()),
        };
        let effect_seed = picker.randint(0, i64::MAX / 2) as u64;
        let mut p = Producer::start(&input, &name, effect_seed, args.fps, args.cols, args.rows)?;
        let mut meta = GlyphFrame { epoch, ..Default::default() };
        set_frame_name(&mut meta, &name);
        eprintln!("organon-glyphs: [{epoch}] {name} (seed {effect_seed})");

        // Motion: one publish per tick, paced by a drift-free deadline.
        let mut next = Instant::now();
        while p.step() {
            let (cols, rows) = p.walk(&mut cells);
            // Nominal period, never the measured one: a seed reproduces a run.
            persist.apply(&mut cells, cols, rows, period.as_secs_f32());
            meta.cols = cols;
            meta.rows = rows;
            meta.tick = p.tick;
            meta.flags = 0;
            writer.publish(&meta, &cells).map_err(|e| format!("publish: {e}"))?;
            if !args.no_pace {
                next += period;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    // Fell behind (a sleep overshoot, a paused machine): resync rather
                    // than bursting to catch up, which would look like the effect
                    // skipping.
                    next = now;
                }
            }
        }

        // Settle + dwell: the last grid is the held one (§8 — colour keeps moving to
        // the last frame, so it is the FINAL walk that is the still). Republish it as
        // a heartbeat; `generation` does not move because the payload does not.
        //
        // T11: **the effect has settled when the SOURCE has.** `FRAME_SETTLED` is set
        // here whatever the phosphors are doing, so a trail can never hold the settle
        // off — but the trails keep decaying through the dwell (each heartbeat re-walks
        // the settled source and advances them by the heartbeat interval), so the
        // payload keeps changing, `generation` keeps moving, and T5's accumulation keeps
        // restarting, until the last trail crosses the floor (~7τ from a full-white
        // cell). That is the right order: the tracer converges on the picture once the
        // picture has stopped changing, and the world learns that from the counter it
        // already watches. The settle publish itself is the same instant as the last
        // motion frame, so it advances the phosphors by zero and republishes the same
        // trails — with persistence off this is byte-identical to before.
        //
        // W16: this publish and every heartbeat below carry the SAME `meta.tick` as the
        // last motion frame, and that is a contract, not an accident — `tick` is the
        // producer's clock on the wire (`GlyphFrame::tick`), and a republish at the same
        // tick is what `glyph_ring::classify_arrival` calls a `Heartbeat`: the world
        // replaces its picture without restarting the slide in progress or rotating
        // its previous grid. Stamp a heartbeat with a fresh tick and the settle frame
        // would cut the last tick of motion short and every dwell beat would start a
        // slide that goes nowhere.
        let (cols, rows) = p.walk(&mut cells);
        persist.apply(&mut cells, cols, rows, 0.0);
        meta.cols = cols;
        meta.rows = rows;
        meta.tick = p.tick;
        meta.flags = FRAME_SETTLED;
        writer.publish(&meta, &cells).map_err(|e| format!("publish: {e}"))?;
        eprintln!("organon-glyphs: [{epoch}] {name} settled after {} ticks", p.tick);
        if !args.no_pace {
            let until = Instant::now() + Duration::from_secs_f64(args.dwell.max(0.0));
            while Instant::now() < until {
                std::thread::sleep(beat.min(until.saturating_duration_since(Instant::now())));
                if persist.enabled() {
                    // The source is settled and the engine is not ticked, so the walk
                    // is the same grid every time; only the phosphors move. Nominal
                    // interval, as in the motion loop.
                    p.walk(&mut cells);
                    persist.apply(&mut cells, cols, rows, beat.as_secs_f32());
                }
                writer.publish(&meta, &cells).map_err(|e| format!("publish: {e}"))?;
            }
        }

        previous = Some(name);
        epoch = epoch.wrapping_add(1);
        if args.once {
            return Ok(());
        }
    }
}
