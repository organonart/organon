//! # The legibility gate — T2's harness over a frame the renderer produced
//!
//! **PBR text Tier 13 (organon#217).** [`crate::legibility`] makes `doc/pbr_text_engine.md`
//! §9's two laws a number; this module is the half that turns that number into a *gate a
//! person can re-run*: the pieces `verify.sh --legibility` and the `legibility-gate`
//! binary (`src/bin/legibility_gate.rs`) need that `legibility.rs` deliberately does not
//! carry — a thresholds **file**, a way to find the grid in a frame nobody painted, the
//! fixture taken **from the ring** rather than from a file, a determinism check between
//! two frames of the same held render, and the exit code. Everything here is pure CPU and
//! `tests/legibility_gate.rs` runs it end to end over the synthetic painter, binary
//! included; what no test here can say is what a *real* frame scores, which is exactly the
//! step the script exists to run on a GPU.
//!
//! ## What the gate reads, and the caveat that comes with it
//!
//! `organon snap` writes the visual's **production texture** through `snap.rs`: an
//! `Rgba16Float` frame is **Reinhard-tonemapped** (`x / (1 + x)`) and then sRGB-encoded to
//! 8 bits; an 8-bit swapchain is written verbatim. So the PNG the gate scores is the
//! *display* frame, not the HDR buffer T2 asked for. ⚠️ What that costs is specific: a
//! phosphor gain above 1 (the `faceplate` rung's `glyph_gain` is 3 in SDR-white units) is
//! **compressed, not clipped** — a monotone map, so the *ranking* of cells survives and
//! Pearson over a one-colour fixture barely moves, but the *gradient's shape* inside the
//! text (`correlation_lit`, the number that told a 6× gain from a 1× gain through `f32`
//! and could not through bytes) is squashed toward flat, and a lit-cell luma of 3.0 lands
//! at 0.75. The numbers the gate prints are of the frame a person would see, and they
//! say so on their first line. A float readback would need `snap.rs` to learn a second
//! format; that is the world's file, not this tier's.
//!
//! ## Finding the grid
//!
//! [`GridGeom::fit`] assumes the grid touches the frame along one axis. A held-camera
//! frame does not quite: the rig fits the *bounds* (backplane margin included) at the
//! bounds' centre plane, and the tiles' front faces sit nearer the camera than that plane,
//! so even with the margin at zero the projected grid is a fraction of a percent larger
//! than the fit and slides by up to a fifth of a cell at the edges of an 81-column logo.
//! [`locate`] absorbs this: a centred scale sweep, then an offset refinement, maximising
//! Pearson against the fixture. It is a search over three numbers on a landscape with one
//! sharp peak (a grid of 810 cells aligned to itself), so the maximum is the alignment and
//! not a flattering local optimum — the synthetic test pins that a grid padded on both
//! axes, which `fit` misplaces, is recovered to within a twentieth of a pixel. The
//! geometry found is printed so a person can pin it with `--geom X,Y,W` next time.
//!
//! ## The fixture from the ring
//!
//! §9's law 2 is "the cell's brightness tracks *what TTE said that cell was*", and what
//! TTE said is on the glyph ring, not in a file: an effect's settled frame carries its own
//! `final_gradient` colours, which the hand-written logo fixture (one colour, deliberately
//! — it is a *shape* census) does not know. With `--ring <ns>` the gate reads the settled
//! grid the producer published, checks its **shape** against the fixture file (same
//! dimensions, same symbol in every cell — the cross-check that the producer drew the
//! text the fixture describes), and scores the frame against the ring's **colours**. A
//! ring that is not settled is refused: gating a moving grid is not a measurement.
//!
//! ## Determinism
//!
//! The settled frame is path-traced (T5's handover, `world.rs::pathtrace_active`) and
//! accumulates, so two snaps seconds apart are two noise realisations of one picture. The
//! per-cell box filter averages hundreds of pixels per cell, which is why the *numbers*
//! agree to well inside the thresholds while the *pixels* do not; `--second <png>` scores
//! a second frame at the **same** geometry and reports the largest difference across the
//! three judged numbers as `spread`, against `max_spread` from the thresholds file. That
//! is the determinism claim made measurable inside the run rather than asserted.

use crate::legibility::{
    assess, downsample, linear_to_srgb_f, pearson, Cell, Fixture, GridGeom, Image, Report,
    Thresholds, ASPECT_TTE,
};
use organon_core::glyph_ring::{unpack_rgb, GlyphGrid, GlyphRingReader, SGR_HAS_FG};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Every law held (and, with `--second`, the spread too).
pub const EXIT_PASS: i32 = 0;
/// A measurement was made and a threshold was not met.
pub const EXIT_FAIL: i32 = 1;
/// The gate could not measure: bad usage, a file that would not read, a ring that was not
/// settled. Never conflated with a failing report — a harness must tell the two apart.
pub const EXIT_USAGE: i32 = 2;

/// The thresholds file: T2's three laws plus the determinism spread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateThresholds {
    pub laws: Thresholds,
    /// With a second frame: the largest |Δ| over `correlation`, `bleed_max` and
    /// `stray_fraction` between the two must be at most this.
    pub max_spread: f32,
}

impl Default for GateThresholds {
    fn default() -> Self {
        GateThresholds { laws: Thresholds::default(), max_spread: 0.02 }
    }
}

/// Parse the thresholds file — `key = value` lines, `#` comments, `[section]` headers
/// ignored. **All four keys are required** and an unknown key is an error: a threshold is
/// a test's opinion, and an opinion that silently fell back to a default is not one.
pub fn parse_thresholds(text: &str) -> Result<GateThresholds, String> {
    let mut min_correlation = None;
    let mut max_bleed = None;
    let mut max_stray = None;
    let mut max_spread = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() || (line.starts_with('[') && line.ends_with(']')) {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("thresholds line {}: expected `key = value`, got `{}`", i + 1, line));
        };
        let key = k.trim();
        let value: f32 = v
            .trim()
            .parse()
            .map_err(|e| format!("thresholds line {}: `{}` is not a number ({e})", i + 1, v.trim()))?;
        if !value.is_finite() {
            return Err(format!("thresholds line {}: `{key}` must be finite", i + 1));
        }
        let slot = match key {
            "min_correlation" => &mut min_correlation,
            "max_bleed" => &mut max_bleed,
            "max_stray" => &mut max_stray,
            "max_spread" => &mut max_spread,
            other => {
                return Err(format!(
                    "thresholds line {}: unknown key `{other}` (want min_correlation, max_bleed, max_stray, max_spread)",
                    i + 1
                ))
            }
        };
        if slot.is_some() {
            return Err(format!("thresholds line {}: `{key}` given twice", i + 1));
        }
        *slot = Some(value);
    }
    let need = |name: &str, v: Option<f32>| v.ok_or_else(|| format!("thresholds file: `{name}` is missing"));
    let t = GateThresholds {
        laws: Thresholds {
            min_correlation: need("min_correlation", min_correlation)?,
            max_bleed: need("max_bleed", max_bleed)?,
            max_stray: need("max_stray", max_stray)?,
        },
        max_spread: need("max_spread", max_spread)?,
    };
    if !(-1.0..=1.0).contains(&t.laws.min_correlation) {
        return Err(format!("thresholds file: min_correlation {} is not a correlation (-1..=1)", t.laws.min_correlation));
    }
    for (name, v) in [("max_bleed", t.laws.max_bleed), ("max_stray", t.laws.max_stray), ("max_spread", t.max_spread)] {
        if v < 0.0 {
            return Err(format!("thresholds file: {name} {v} is negative"));
        }
    }
    Ok(t)
}

/// How the gate finds the grid in the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GeomMode {
    /// [`GridGeom::fit`] — the largest centred grid at the fixture's aspect.
    Fit,
    /// [`locate`] — search for the alignment that maximises correlation.
    Auto,
    /// A geometry handed in: pixel origin of cell `(0, 0)` and the cell width; the cell
    /// height follows the fixture's aspect.
    Explicit { origin: [f32; 2], cell_w: f32 },
}

/// `fit` | `auto` | `X,Y,W`.
pub fn parse_geom(arg: &str) -> Result<GeomMode, String> {
    match arg.trim() {
        "fit" => Ok(GeomMode::Fit),
        "auto" => Ok(GeomMode::Auto),
        s => {
            let parts: Vec<&str> = s.split(',').map(str::trim).collect();
            if parts.len() != 3 {
                return Err(format!("--geom wants `fit`, `auto` or `X,Y,W` (pixels), got `{s}`"));
            }
            let num = |p: &str| p.parse::<f32>().map_err(|_| format!("--geom: `{p}` is not a number"));
            let (x, y, w) = (num(parts[0])?, num(parts[1])?, num(parts[2])?);
            if !(w > 0.0) || !x.is_finite() || !y.is_finite() {
                return Err(format!("--geom: cell width must be positive and the origin finite, got `{s}`"));
            }
            Ok(GeomMode::Explicit { origin: [x, y], cell_w: w })
        }
    }
}

fn centred(width: usize, height: usize, fixture: &Fixture, cell_w: f32) -> GridGeom {
    let cell_h = cell_w * fixture.aspect;
    GridGeom {
        origin: [
            (width as f32 - cell_w * fixture.cols as f32) * 0.5,
            (height as f32 - cell_h * fixture.rows as f32) * 0.5,
        ],
        cell_w,
        cell_h,
    }
}

fn score(image: &Image, geom: &GridGeom, fixture: &Fixture, expected: &[f32]) -> f32 {
    let c = pearson(&downsample(image, geom, fixture.cols, fixture.rows), expected);
    if c.is_nan() { f32::NEG_INFINITY } else { c }
}

/// Find the grid: the centred scale (30 % … 102 % of the [`GridGeom::fit`] cell width, coarse
/// then fine) and then the origin (a cell's reach at a coarse step, then half-pixel, then
/// a tenth) that maximise Pearson against the fixture. Returns the geometry and the
/// correlation there. Deterministic; some five hundred box-filter passes over the frame —
/// a second or two on a 1080p snap in release. A grid more than one cell off centre is
/// outside the reach; hand it `--geom X,Y,W` instead.
pub fn locate(image: &Image, fixture: &Fixture) -> (GridGeom, f32) {
    let expected = fixture.expected_luma();
    let fit = GridGeom::fit(image.width, image.height, fixture);
    let eval = |g: &GridGeom| score(image, g, fixture, &expected);

    // Scale, centred — coarse.
    let mut best_s = 1.0f32;
    let mut best = eval(&fit);
    let mut s = 0.30f32;
    while s <= 1.02 {
        let c = eval(&centred(image.width, image.height, fixture, fit.cell_w * s));
        if c > best {
            best = c;
            best_s = s;
        }
        s += 0.02;
    }
    // Scale — fine, around the coarse winner.
    let (lo, hi) = (best_s - 0.03, best_s + 0.03);
    let mut s = lo;
    while s <= hi {
        let c = eval(&centred(image.width, image.height, fixture, fit.cell_w * s));
        if c > best {
            best = c;
            best_s = s;
        }
        s += 0.002;
    }
    let mut geom = centred(image.width, image.height, fixture, fit.cell_w * best_s);
    // Origin — three passes, each a square of offsets around the current best: a cell's
    // reach at a coarse step (so a grid up to one cell off centre is found), then the
    // coarse step's reach at half a pixel, then half a pixel at a tenth.
    let coarse = (geom.cell_w / 8.0).max(1.0);
    for (reach_x, reach_y, step) in [(geom.cell_w, geom.cell_h, coarse), (coarse, coarse, 0.5), (0.5, 0.5, 0.1)] {
        let base = geom;
        let (nx, ny) = ((reach_x / step).round() as i32, (reach_y / step).round() as i32);
        for dy in -ny..=ny {
            for dx in -nx..=nx {
                let g = GridGeom {
                    origin: [base.origin[0] + dx as f32 * step, base.origin[1] + dy as f32 * step],
                    ..base
                };
                let c = eval(&g);
                if c > best {
                    best = c;
                    geom = g;
                }
            }
        }
    }
    (geom, best)
}

/// A fixture built from a glyph-ring grid — *what TTE said*, taken from the wire. A cell
/// with no symbol, or one that draws nothing (a space — ttfx paints spaces, sometimes with
/// a colour), is [`Cell::BLANK`] exactly as the file parser makes one, so a ring and a
/// file of the same text compare equal cell for cell; a lit cell with no foreground
/// (`SGR_HAS_FG` clear) takes `default_fg`, the renderer's own rule for such a cell. The
/// aspect is the ring's.
pub fn fixture_from_grid(grid: &GlyphGrid, default_fg: [u8; 3]) -> Result<Fixture, String> {
    let (cols, rows) = (grid.cols(), grid.rows());
    if cols == 0 || rows == 0 || grid.cells.len() != cols * rows {
        return Err(format!(
            "ring grid is {cols}x{rows} with {} cells — nothing to build a fixture from",
            grid.cells.len()
        ));
    }
    let cells = grid
        .cells
        .iter()
        .map(|c| {
            if c.symbol == 0 {
                return Cell::BLANK;
            }
            let symbol = char::from_u32(c.symbol).unwrap_or('\u{FFFD}');
            let fg = if c.sgr & SGR_HAS_FG != 0 { unpack_rgb(c.fg) } else { default_fg };
            let cell = Cell { symbol, fg };
            if cell.is_lit() { cell } else { Cell::BLANK }
        })
        .collect();
    let aspect = if grid.cell_aspect > 0.0 && grid.cell_aspect.is_finite() { grid.cell_aspect } else { ASPECT_TTE };
    Ok(Fixture::from_cells(cols, rows, aspect, cells))
}

/// The cross-check that the producer drew the fixture's text: same dimensions, and the
/// same **symbol** in every cell (colour is exactly what the ring is allowed to differ
/// on). The message names the first cell that disagrees and how many do.
pub fn shape_agrees(file: &Fixture, ring: &Fixture) -> Result<(), String> {
    if (file.cols, file.rows) != (ring.cols, ring.rows) {
        return Err(format!(
            "the ring is {}x{} but the fixture is {}x{} — the producer did not draw this fixture's grid",
            ring.cols, ring.rows, file.cols, file.rows
        ));
    }
    let mut first = None;
    let mut count = 0usize;
    for r in 0..file.rows {
        for c in 0..file.cols {
            let (a, b) = (file.cell(c, r).symbol, ring.cell(c, r).symbol);
            if a != b {
                count += 1;
                if first.is_none() {
                    first = Some((c, r, a, b));
                }
            }
        }
    }
    match first {
        None => Ok(()),
        Some((c, r, a, b)) => Err(format!(
            "the ring's text is not the fixture's: {count} cell(s) differ, first at ({c},{r}) — fixture `{a}`, ring `{b}`"
        )),
    }
}

/// The producer's input, derived from the fixture rather than kept as a second copy: the
/// symbol rows top-down, trailing blanks trimmed (the source is ragged; the fixture's
/// fences made the padding explicit and this takes it back out). A producer run over this
/// text with `--cols`/`--rows` from the same fixture publishes the fixture's grid.
pub fn emit_text(fixture: &Fixture) -> String {
    let mut out = String::new();
    for r in 0..fixture.rows {
        let row: String = (0..fixture.cols).map(|c| fixture.cell(c, r).symbol).collect();
        out.push_str(row.trim_end_matches(' '));
        out.push('\n');
    }
    out
}

/// Parse `#rrggbb` or a linear grey `0..=1` into sRGB bytes — the `--default-fg` option.
pub fn parse_default_fg(arg: &str) -> Result<[u8; 3], String> {
    let s = arg.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() != 6 {
            return Err(format!("--default-fg: `{s}` is not #rrggbb"));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| format!("--default-fg: `{s}` is not #rrggbb"));
        return Ok([byte(0)?, byte(2)?, byte(4)?]);
    }
    let grey: f32 = s.parse().map_err(|_| format!("--default-fg: `{s}` is neither #rrggbb nor a linear grey"))?;
    if !(0.0..=1.0).contains(&grey) {
        return Err(format!("--default-fg: linear grey {grey} is outside 0..=1"));
    }
    let b = (linear_to_srgb_f(grey) * 255.0).round().clamp(0.0, 255.0) as u8;
    Ok([b, b, b])
}

/// Where the ring comes from, when it does.
#[derive(Clone, Debug, PartialEq)]
pub enum RingSource {
    /// An IPC namespace (`$ORGANON_IPC_NS`'s rule).
    Namespace(String),
    /// An explicit ring file (tests).
    File(PathBuf),
}

/// One gate run, parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct GateArgs {
    pub image: PathBuf,
    pub fixture: PathBuf,
    pub second: Option<PathBuf>,
    pub thresholds: Option<PathBuf>,
    pub geom: GeomMode,
    pub ring: Option<RingSource>,
    /// sRGB bytes for a ring cell with a symbol and no colour (`GlyphLook::default_fg`,
    /// linear 0.75, by default).
    pub default_fg: [u8; 3],
    /// Write the per-cell measured luma as a `cols × rows` PNG — the report as a picture.
    pub dump: Option<PathBuf>,
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Gate(GateArgs),
    /// `--emit-text <fixture>`: print the producer input and exit.
    EmitText(PathBuf),
    Help,
}

pub const USAGE: &str = "\
legibility-gate <frame.png> <fixture.txt> [options]
legibility-gate --emit-text <fixture.txt>

  Score a rendered frame against a legibility fixture (doc/pbr_text_engine.md §9) and
  exit 0 (pass), 1 (a threshold not met) or 2 (could not measure).

  --thresholds <file>   key = value lines: min_correlation, max_bleed, max_stray,
                        max_spread (all required); default = legibility::Thresholds
  --geom fit|auto|X,Y,W where the grid is: the largest centred fit, a search, or
                        the pixel origin of cell (0,0) and the cell width (default auto)
  --second <frame.png>  a second frame of the same held render, scored at the same
                        geometry; the spread of the numbers is judged against max_spread
  --ring <ns>           take the fixture's colours from the settled glyph ring in this
                        IPC namespace, after checking its shape against the fixture file
  --ring-file <path>    the same, from an explicit ring file
  --default-fg <v>      #rrggbb or a linear grey for a ring cell with no colour (0.75)
  --dump <cells.png>    write the measured per-cell luma as a cols x rows picture
  --emit-text <fixture> print the producer input derived from the fixture and exit
";

/// Parse the command line (without the program name).
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut positional: Vec<PathBuf> = Vec::new();
    let mut second = None;
    let mut thresholds = None;
    let mut geom = GeomMode::Auto;
    let mut ring = None;
    let mut default_fg = parse_default_fg("0.75")?;
    let mut dump = None;
    let mut i = 0;
    let value = |i: &mut usize, flag: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| format!("{flag} wants a value"))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => return Ok(Command::Help),
            "--emit-text" => return Ok(Command::EmitText(PathBuf::from(value(&mut i, a)?))),
            "--second" => second = Some(PathBuf::from(value(&mut i, a)?)),
            "--thresholds" => thresholds = Some(PathBuf::from(value(&mut i, a)?)),
            "--geom" => geom = parse_geom(&value(&mut i, a)?)?,
            "--ring" => ring = Some(RingSource::Namespace(value(&mut i, a)?)),
            "--ring-file" => ring = Some(RingSource::File(PathBuf::from(value(&mut i, a)?))),
            "--default-fg" => default_fg = parse_default_fg(&value(&mut i, a)?)?,
            "--dump" => dump = Some(PathBuf::from(value(&mut i, a)?)),
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`")),
            _ => positional.push(PathBuf::from(a)),
        }
        i += 1;
    }
    if positional.len() != 2 {
        return Err(format!("want <frame.png> <fixture.txt>, got {} positional argument(s)", positional.len()));
    }
    Ok(Command::Gate(GateArgs {
        image: positional[0].clone(),
        fixture: positional[1].clone(),
        second,
        thresholds,
        geom,
        ring,
        default_fg,
        dump,
    }))
}

/// Read a PNG (or anything `image` decodes) into linear light.
pub fn load_image(path: &Path) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let rgba = img.to_rgba8();
    Ok(Image::from(&rgba))
}

/// Write the per-cell measured luma as a `cols × rows` 8-bit PNG, normalised to the
/// brightest cell so a dim frame still reads. Purely for a person's eyes.
pub fn dump_cells(report: &Report, path: &Path) -> Result<(), String> {
    let peak = report.measured.iter().cloned().fold(0.0f32, f32::max);
    let mut px = Vec::with_capacity(report.cols * report.rows * 4);
    for m in &report.measured {
        let v = if peak > 0.0 { m / peak } else { 0.0 };
        let b = (linear_to_srgb_f(v) * 255.0).round().clamp(0.0, 255.0) as u8;
        px.extend_from_slice(&[b, b, b, 255]);
    }
    let img = image::RgbaImage::from_raw(report.cols as u32, report.rows as u32, px)
        .ok_or_else(|| "cell dump: buffer size disagrees with the grid".to_string())?;
    img.save(path).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// The largest difference over the three judged numbers between two reports.
pub fn spread(a: &Report, b: &Report) -> f32 {
    let d = |x: f32, y: f32| if x.is_nan() || y.is_nan() { f32::NAN } else { (x - y).abs() };
    let s = [
        d(a.correlation, b.correlation),
        d(a.bleed_max, b.bleed_max),
        d(a.stray_fraction, b.stray_fraction),
    ];
    if s.iter().any(|v| v.is_nan()) { f32::NAN } else { s.iter().cloned().fold(0.0, f32::max) }
}

fn read_ring(source: &RingSource) -> Result<GlyphGrid, String> {
    let reader = match source {
        RingSource::Namespace(ns) => GlyphRingReader::open_ns(ns)?,
        RingSource::File(p) => GlyphRingReader::open_at(p),
    };
    if !reader.is_open() {
        // The line a person reads when `verify.sh --legibility` finds no ring: name the
        // namespace or the path plainly, not the enum's Debug shape (review nit on #240).
        return Err(match source {
            RingSource::Namespace(ns) => {
                format!("no glyph ring in IPC namespace `{ns}` — is organon-glyphs running with ORGANON_IPC_NS={ns}?")
            }
            RingSource::File(p) => format!("no glyph ring file at {} — nothing has written it", p.display()),
        });
    }
    let mut grid = GlyphGrid::default();
    if !reader.latest_into(&mut grid) {
        return Err("the glyph ring exists but holds no frame (or its layout is not this build's)".into());
    }
    if !grid.settled() {
        return Err(format!(
            "the glyph ring is not settled (tick {}, generation {}) — gating a moving grid is not a measurement; wait for the producer's settle",
            grid.frame.tick, grid.frame.generation
        ));
    }
    Ok(grid)
}

/// Run the gate. Writes the report to `out`; returns the exit code, or `Err` for anything
/// that stopped a measurement being made (the caller maps that to [`EXIT_USAGE`]).
pub fn run(args: &GateArgs, out: &mut String) -> Result<i32, String> {
    let fixture_text = std::fs::read_to_string(&args.fixture)
        .map_err(|e| format!("reading fixture {}: {e}", args.fixture.display()))?;
    let file_fixture = Fixture::parse(&fixture_text).map_err(|e| format!("fixture {}: {e}", args.fixture.display()))?;
    let thresholds = match &args.thresholds {
        Some(p) => {
            let t = std::fs::read_to_string(p).map_err(|e| format!("reading thresholds {}: {e}", p.display()))?;
            parse_thresholds(&t).map_err(|e| format!("{}: {e}", p.display()))?
        }
        None => GateThresholds::default(),
    };

    let fixture = match &args.ring {
        None => file_fixture.clone(),
        Some(src) => {
            let grid = read_ring(src)?;
            let ring_fixture = fixture_from_grid(&grid, args.default_fg)?;
            shape_agrees(&file_fixture, &ring_fixture)?;
            let _ = writeln!(
                out,
                "fixture: shape from {} · colours from the settled ring (effect `{}`, generation {}, {} lit cells)",
                args.fixture.display(),
                organon_core::glyph_ring::frame_name(&grid.frame),
                grid.frame.generation,
                ring_fixture.lit_count()
            );
            ring_fixture
        }
    };
    if args.ring.is_none() {
        let _ = writeln!(out, "fixture: {} ({}x{}, {} lit cells)", args.fixture.display(), fixture.cols, fixture.rows, fixture.lit_count());
    }

    let image = load_image(&args.image)?;
    let _ = writeln!(
        out,
        "frame: {} ({}x{}) — 8-bit sRGB as `organon snap` writes it: the display frame after the visual's tonemap, so a gain above 1 is compressed, not linear",
        args.image.display(),
        image.width,
        image.height
    );
    let peak = image.rgb.iter().flat_map(|p| p.iter()).cloned().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        let _ = writeln!(out, "⚠️ the frame is black — nothing rendered, or the snap caught an occluded window");
    }

    let geom = match args.geom {
        GeomMode::Fit => GridGeom::fit(image.width, image.height, &fixture),
        GeomMode::Explicit { origin, cell_w } => GridGeom::at(origin, cell_w, fixture.aspect),
        GeomMode::Auto => locate(&image, &fixture).0,
    };
    let _ = writeln!(
        out,
        "geometry ({}): origin ({:.2}, {:.2}) px · cell {:.3} x {:.3} px · pin it with --geom {:.2},{:.2},{:.3}",
        match args.geom {
            GeomMode::Fit => "fit",
            GeomMode::Auto => "auto",
            GeomMode::Explicit { .. } => "explicit",
        },
        geom.origin[0],
        geom.origin[1],
        geom.cell_w,
        geom.cell_h,
        geom.origin[0],
        geom.origin[1],
        geom.cell_w
    );

    let report = assess(&image, &geom, &fixture, thresholds.laws);
    let _ = writeln!(out, "{}", report.summary());
    if let Some(p) = &args.dump {
        dump_cells(&report, p)?;
        let _ = writeln!(out, "cells: {}", p.display());
    }

    let mut ok = report.pass();
    let mut failed: Vec<String> = Vec::new();
    if !report.correlation_ok() {
        failed.push(format!("correlation {:.4} < {:.2}", report.correlation, thresholds.laws.min_correlation));
    }
    if !report.bleed_ok() {
        failed.push(format!("bleed {:.3} > {:.2}", report.bleed_max, thresholds.laws.max_bleed));
    }
    if !report.stray_ok() {
        failed.push(format!("stray {:.4} > {:.2}", report.stray_fraction, thresholds.laws.max_stray));
    }

    if let Some(second) = &args.second {
        let image_b = load_image(second)?;
        if (image_b.width, image_b.height) != (image.width, image.height) {
            return Err(format!(
                "--second {} is {}x{} but the first frame is {}x{}",
                second.display(),
                image_b.width,
                image_b.height,
                image.width,
                image.height
            ));
        }
        let report_b = assess(&image_b, &geom, &fixture, thresholds.laws);
        let _ = writeln!(out, "second: {}", second.display());
        let _ = writeln!(out, "{}", report_b.summary());
        let s = spread(&report, &report_b);
        let spread_ok = s <= thresholds.max_spread;
        let _ = writeln!(
            out,
            "spread: {:.4} (<= {:.2} {}) — the largest change over corr/bleed/stray between the two frames",
            s,
            thresholds.max_spread,
            if spread_ok { "ok" } else { "FAIL" }
        );
        if !spread_ok {
            ok = false;
            failed.push(format!("spread {s:.4} > {:.2} (the two frames do not agree)", thresholds.max_spread));
        }
        if !report_b.pass() {
            ok = false;
            failed.push("the second frame fails on its own".into());
        }
    }

    if ok {
        let _ = writeln!(out, "legibility-gate: PASS");
        Ok(EXIT_PASS)
    } else {
        let _ = writeln!(out, "legibility-gate: FAIL — {}", failed.join("; "));
        Ok(EXIT_FAIL)
    }
}
