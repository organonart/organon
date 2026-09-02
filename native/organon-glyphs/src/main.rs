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
use organon_glyphs::{effect_names, pick_next, Producer};
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
        let (cols, rows) = p.walk(&mut cells);
        meta.cols = cols;
        meta.rows = rows;
        meta.tick = p.tick;
        meta.flags = FRAME_SETTLED;
        writer.publish(&meta, &cells).map_err(|e| format!("publish: {e}"))?;
        eprintln!("organon-glyphs: [{epoch}] {name} settled after {} ticks", p.tick);
        if !args.no_pace {
            let until = Instant::now() + Duration::from_secs_f64(args.dwell.max(0.0));
            let beat = Duration::from_millis(args.heartbeat_ms.max(10));
            while Instant::now() < until {
                std::thread::sleep(beat.min(until.saturating_duration_since(Instant::now())));
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
