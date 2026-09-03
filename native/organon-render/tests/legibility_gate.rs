//! **The legibility gate, end to end and without a GPU** (PBR text T13, organon#217).
//!
//! `src/legibility_gate.rs` is what turns T2's number into something `verify.sh` can run
//! and a person can re-run; this file proves each piece against inputs whose right answer
//! is known, using T2's own synthetic painter for the frames and a glyph ring written to a
//! temp file for the wire:
//!
//! - the **thresholds file** parses, and every way it can be wrong is named;
//! - the **producer input** derived from a fixture reproduces the fixture's grid;
//! - the **fixture from the ring** carries the wire's colours and the file's shape, refuses
//!   an unsettled ring, and names the cell where the text disagrees;
//! - **`locate`** recovers a grid that `GridGeom::fit` misplaces, and does not wander when
//!   `fit` was already right;
//! - the **binary** exits 0 on a render that passes, 1 naming the term that failed, 1 on a
//!   spread between two frames, and 2 when it could not measure.
//!
//! ⚠️ What none of this can say is what a real frame scores. That is the run
//! `verify.sh --legibility` makes on a GPU, and `native/verify/README.md` says what a pass
//! looks like there.

use organon_core::glyph_ring::{
    pack_rgb, GlyphCell, GlyphFrame, GlyphRingReader, GlyphRingWriter, FRAME_SETTLED, SGR_HAS_FG,
    TTFX_CELL_ASPECT,
};
use organon_render::legibility::{assess, synth, Cell, Fixture, GridGeom, Image, Thresholds};
use organon_render::legibility_gate::{
    emit_text, fixture_from_grid, locate, parse_args, parse_geom, parse_thresholds, run,
    shape_agrees, spread, Command, GateArgs, GateThresholds, GeomMode, RingSource, EXIT_FAIL,
    EXIT_PASS, EXIT_USAGE,
};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

const LOGO: &str = include_str!("fixtures/omarchy-logo.txt");
const ASYM: &str = include_str!("fixtures/asymmetric.txt");
const LOGO_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/omarchy-logo.txt");

fn logo() -> Fixture {
    Fixture::parse(LOGO).unwrap()
}
fn asym() -> Fixture {
    Fixture::parse(ASYM).unwrap()
}

/// A fresh scratch directory per test, so parallel tests never share a file.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("organon-legibility-gate-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_png(img: &Image, path: &Path) {
    let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.to_rgba8_srgb()).unwrap();
    rgba.save(path).unwrap();
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

const GOOD_THRESHOLDS: &str = "\
# a comment
[thresholds]
min_correlation = 0.90   # trailing comment
max_bleed = 0.25
max_stray = 0.10
max_spread = 0.02
";

// ---------------------------------------------------------------------------------------
// The thresholds file
// ---------------------------------------------------------------------------------------

#[test]
fn thresholds_file_parses_and_names_what_is_wrong() {
    let t = parse_thresholds(GOOD_THRESHOLDS).unwrap();
    assert_eq!(t, GateThresholds { laws: Thresholds { min_correlation: 0.90, max_bleed: 0.25, max_stray: 0.10 }, max_spread: 0.02 });
    // The committed file is the one the script reads: it must parse, and it must be T2's
    // defaults until a GPU run tightens it.
    let committed = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../verify/legibility/thresholds.toml")).unwrap();
    assert_eq!(parse_thresholds(&committed).unwrap().laws, Thresholds::default(), "verify/legibility/thresholds.toml drifted from T2's defaults without a GPU run saying so");

    let missing = parse_thresholds("min_correlation = 0.9\nmax_bleed = 0.2\nmax_stray = 0.1\n").unwrap_err();
    assert!(missing.contains("max_spread") && missing.contains("missing"), "{missing}");
    let unknown = parse_thresholds(&format!("{GOOD_THRESHOLDS}min_corelation = 0.5\n")).unwrap_err();
    assert!(unknown.contains("unknown key `min_corelation`"), "{unknown}");
    let twice = parse_thresholds(&format!("{GOOD_THRESHOLDS}max_bleed = 0.3\n")).unwrap_err();
    assert!(twice.contains("`max_bleed` given twice"), "{twice}");
    let junk = parse_thresholds("min_correlation = high\n").unwrap_err();
    assert!(junk.contains("line 1") && junk.contains("not a number"), "{junk}");
    let shape = parse_thresholds("min_correlation 0.9\n").unwrap_err();
    assert!(shape.contains("expected `key = value`"), "{shape}");
    let range = parse_thresholds("min_correlation = 1.5\nmax_bleed = 0.2\nmax_stray = 0.1\nmax_spread = 0.01\n").unwrap_err();
    assert!(range.contains("not a correlation"), "{range}");
    let neg = parse_thresholds("min_correlation = 0.9\nmax_bleed = -0.2\nmax_stray = 0.1\nmax_spread = 0.01\n").unwrap_err();
    assert!(neg.contains("max_bleed") && neg.contains("negative"), "{neg}");
}

#[test]
fn geometry_argument_parses_its_three_forms() {
    assert_eq!(parse_geom("fit").unwrap(), GeomMode::Fit);
    assert_eq!(parse_geom(" auto ").unwrap(), GeomMode::Auto);
    assert_eq!(parse_geom("12.5, 4, 8.25").unwrap(), GeomMode::Explicit { origin: [12.5, 4.0], cell_w: 8.25 });
    assert!(parse_geom("1,2").unwrap_err().contains("X,Y,W"));
    assert!(parse_geom("1,2,0").unwrap_err().contains("positive"));
    assert!(parse_geom("1,two,3").unwrap_err().contains("not a number"));
}

#[test]
fn the_command_line_parses_and_refuses_what_it_does_not_know() {
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match parse_args(&s(&["a.png", "f.txt", "--second", "b.png", "--geom", "fit", "--ring", "ns1", "--default-fg", "#ff8000"])).unwrap() {
        Command::Gate(a) => {
            assert_eq!(a.image, PathBuf::from("a.png"));
            assert_eq!(a.fixture, PathBuf::from("f.txt"));
            assert_eq!(a.second, Some(PathBuf::from("b.png")));
            assert_eq!(a.geom, GeomMode::Fit);
            assert_eq!(a.ring, Some(RingSource::Namespace("ns1".into())));
            assert_eq!(a.default_fg, [0xff, 0x80, 0x00]);
        }
        other => panic!("{other:?}"),
    }
    // The default for a colourless ring cell is the renderer's own `default_fg`, linear 0.75.
    match parse_args(&s(&["a.png", "f.txt"])).unwrap() {
        Command::Gate(a) => {
            assert_eq!(a.geom, GeomMode::Auto);
            assert_eq!(a.default_fg, [225, 225, 225], "linear 0.75 encodes to sRGB 225");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(parse_args(&s(&["--emit-text", "f.txt"])).unwrap(), Command::EmitText(PathBuf::from("f.txt")));
    assert_eq!(parse_args(&s(&["--help"])).unwrap(), Command::Help);
    assert!(parse_args(&s(&["a.png"])).unwrap_err().contains("got 1 positional"));
    assert!(parse_args(&s(&["a.png", "f.txt", "--treshold", "t"])).unwrap_err().contains("unknown option `--treshold`"));
    assert!(parse_args(&s(&["a.png", "f.txt", "--second"])).unwrap_err().contains("--second wants a value"));
}

// ---------------------------------------------------------------------------------------
// The producer input
// ---------------------------------------------------------------------------------------

#[test]
fn the_emitted_text_is_the_fixture_with_its_padding_taken_back_out() {
    let f = logo();
    let text = emit_text(&f);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), f.rows);
    // The source's ragged widths, restored: 20, 80, 81, 81, 81, 81, 81, 81, 80, 47.
    let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
    assert_eq!(widths, vec![20, 80, 81, 81, 81, 81, 81, 81, 80, 47]);
    // Re-padded, every symbol is where the fixture has it.
    for (r, line) in lines.iter().enumerate() {
        let padded: Vec<char> = line.chars().chain(std::iter::repeat(' ')).take(f.cols).collect();
        for (c, ch) in padded.iter().enumerate() {
            assert_eq!(*ch, f.cell(c, r).symbol, "({c},{r})");
        }
    }
    // Top-down in memory whatever the file's order: the asymmetric fixture is written
    // bottom-up and its "L" has the long stroke at the bottom of the picture.
    let a = asym();
    let t = emit_text(&a);
    let first = t.lines().next().unwrap();
    let last = t.lines().last().unwrap();
    assert!(last.chars().filter(|c| *c != ' ').count() > first.chars().filter(|c| *c != ' ').count(), "the foot of the L is the last line:\n{t}");
}

// ---------------------------------------------------------------------------------------
// The fixture from the ring
// ---------------------------------------------------------------------------------------

/// The logo's cells as a ring would carry them: every lit cell gets its own colour from a
/// gradient (so the ring's colours are NOT the file's one colour), except every third lit
/// cell, which is published with no foreground at all.
fn ring_cells(f: &Fixture) -> Vec<GlyphCell> {
    let mut lit_i = 0usize;
    f.cells()
        .iter()
        .map(|cell| {
            if !cell.is_lit() {
                // A blank: a space with no colour, as ttfx paints one.
                return GlyphCell { symbol: ' ' as u32, ..Default::default() };
            }
            lit_i += 1;
            let t = (lit_i % 97) as f32 / 96.0;
            let (r, g, b) = ((60.0 + 180.0 * t) as u8, (240.0 - 120.0 * t) as u8, 200u8);
            if lit_i % 3 == 0 {
                GlyphCell { symbol: cell.symbol as u32, character_id: lit_i as u32, ..Default::default() }
            } else {
                GlyphCell { symbol: cell.symbol as u32, fg: pack_rgb(r, g, b), sgr: SGR_HAS_FG, character_id: lit_i as u32, ..Default::default() }
            }
        })
        .collect()
}

fn write_ring(path: &Path, f: &Fixture, cells: &[GlyphCell], settled: bool) {
    let mut w = GlyphRingWriter::create_at(path, TTFX_CELL_ASPECT, 120.0).unwrap();
    let meta = GlyphFrame { cols: f.cols as u32, rows: f.rows as u32, flags: if settled { FRAME_SETTLED } else { 0 }, ..Default::default() };
    w.publish(&meta, cells).unwrap();
}

#[test]
fn the_fixture_from_the_ring_has_the_wire_colours_and_the_file_shape() {
    let f = logo();
    let dir = scratch("ring");
    let ring = dir.join("glyphs.bin");
    let cells = ring_cells(&f);
    write_ring(&ring, &f, &cells, true);

    let reader = GlyphRingReader::open_at(&ring);
    let mut grid = Default::default();
    assert!(reader.latest_into(&mut grid));
    let default_fg = [225, 225, 225];
    let rf = fixture_from_grid(&grid, default_fg).unwrap();
    assert_eq!((rf.cols, rf.rows, rf.aspect), (81, 10, 2.0));
    shape_agrees(&f, &rf).expect("the ring drew the fixture's text");
    assert_eq!(rf.lit_count(), f.lit_count());
    // Colours: the wire's where the wire had one, the default where it did not — and
    // never the file's.
    let mut from_wire = 0;
    let mut defaulted = 0;
    for (i, cell) in cells.iter().enumerate() {
        let (c, r) = (i % f.cols, i / f.cols);
        let got = rf.cell(c, r);
        if cell.symbol == ' ' as u32 {
            assert_eq!(*got, Cell::BLANK);
        } else if cell.sgr & SGR_HAS_FG != 0 {
            assert_eq!(got.fg, [((cell.fg >> 16) & 255) as u8, ((cell.fg >> 8) & 255) as u8, (cell.fg & 255) as u8]);
            from_wire += 1;
        } else {
            assert_eq!(got.fg, default_fg);
            defaulted += 1;
        }
    }
    assert!(from_wire > 200 && defaulted > 100, "{from_wire} / {defaulted}");
    assert!(rf.cells().iter().filter(|c| c.is_lit()).all(|c| c.fg != [0xc0, 0xca, 0xf5]), "no cell carries the file's colour");

    // A render painted from the RING fixture passes against it — colours and all.
    let (img, geom) = synth::paint(&rf, 6.0, 0.0);
    let rep = assess(&img, &geom, &rf, Thresholds::default());
    assert!(rep.pass() && rep.correlation > 0.999, "{}", rep.summary());
    // The lit-only coefficient exists now (the ring has many colours) — the file fixture
    // could never ask that question of the logo.
    assert!(rep.correlation_lit.is_some());

    // One symbol changed on the ring → the cross-check names the cell.
    let mut wrong = cells.clone();
    // The first full-block cell of row 3 (the logo's rows are ragged; find one rather
    // than assume a column).
    let col = (0..f.cols).find(|c| f.cell(*c, 3).symbol == '█').unwrap();
    let idx = 3 * f.cols + col;
    wrong[idx].symbol = '▄' as u32;
    write_ring(&ring, &f, &wrong, true);
    let mut grid2 = Default::default();
    assert!(GlyphRingReader::open_at(&ring).latest_into(&mut grid2));
    let err = shape_agrees(&f, &fixture_from_grid(&grid2, default_fg).unwrap()).unwrap_err();
    assert!(err.contains("1 cell(s) differ") && err.contains(&format!("({col},3)")) && err.contains("fixture `█`, ring `▄`"), "{err}");

    // A ring of another size is refused before any cell is compared.
    let small = Fixture::parse("grid v1\ncols 2\nrows 1\naspect 2\norder top-down\ndefault #ffffff\nglyphs\n|█ |\n").unwrap();
    let err = shape_agrees(&f, &small).unwrap_err();
    assert!(err.contains("2x1") && err.contains("81x10"), "{err}");
}

#[test]
fn an_unsettled_ring_is_refused_by_the_gate() {
    let f = logo();
    let dir = scratch("unsettled");
    let ring = dir.join("glyphs.bin");
    write_ring(&ring, &f, &ring_cells(&f), false);
    let (img, _) = synth::paint(&f, 4.0, 0.0);
    let png = dir.join("frame.png");
    write_png(&img, &png);
    let args = GateArgs {
        image: png,
        fixture: PathBuf::from(LOGO_PATH),
        second: None,
        thresholds: None,
        geom: GeomMode::Fit,
        ring: Some(RingSource::File(ring)),
        default_fg: [225; 3],
        dump: None,
    };
    let mut out = String::new();
    let err = run(&args, &mut out).unwrap_err();
    assert!(err.contains("not settled"), "{err}");
}

// ---------------------------------------------------------------------------------------
// Finding the grid
// ---------------------------------------------------------------------------------------

#[test]
fn locate_recovers_a_grid_that_fit_cannot_and_holds_still_when_fit_is_right() {
    // Padded on BOTH axes — the case `GridGeom::fit`'s own doc says it cannot find.
    let a = asym();
    let (img, painted) = synth::paint(&a, 6.0, 20.0);
    let fit = GridGeom::fit(img.width, img.height, &a);
    let fit_corr = assess(&img, &fit, &a, Thresholds::default()).correlation;
    let (found, corr) = locate(&img, &a);
    assert!(corr > fit_corr + 0.05, "auto {corr:.4} must beat fit {fit_corr:.4} on a padded grid");
    assert!(corr > 0.999, "the painted grid, found: corr {corr:.4}");
    assert!((found.cell_w - painted.cell_w).abs() < 0.05, "cell_w {} vs painted {}", found.cell_w, painted.cell_w);
    assert!((found.origin[0] - painted.origin[0]).abs() < 0.11 && (found.origin[1] - painted.origin[1]).abs() < 0.11, "origin {:?} vs painted {:?}", found.origin, painted.origin);

    // Unpadded: `fit` is exact, and `locate` must land on it rather than wander.
    let f = logo();
    let (img, painted) = synth::paint(&f, 4.0, 0.0);
    let (found, corr) = locate(&img, &f);
    assert!(corr > 0.9999, "{corr}");
    assert!((found.cell_w - painted.cell_w).abs() < 0.01 && (found.origin[0] - painted.origin[0]).abs() < 0.11 && (found.origin[1] - painted.origin[1]).abs() < 0.11, "{found:?} vs {painted:?}");

    // OFF-centre: the grid pasted into a larger canvas 5 px left and 9 px up of centre —
    // no centred scale can reach it, so this is the offset refinement's own test (the
    // padded case above is centred and the scale sweep alone would find it). Within one
    // cell (8 × 16 px here), which is the refinement's stated reach.
    let (small, painted) = synth::paint(&a, 8.0, 0.0);
    let mut canvas = Image::black(small.width + 12, small.height + 24);
    let (ox, oy) = (1usize, 3usize);
    for y in 0..small.height {
        for x in 0..small.width {
            canvas.rgb[(y + oy) * canvas.width + x + ox] = small.rgb[y * small.width + x];
        }
    }
    let want = GridGeom { origin: [painted.origin[0] + ox as f32, painted.origin[1] + oy as f32], ..painted };
    let (found, corr) = locate(&canvas, &a);
    assert!(corr > 0.999, "off-centre grid found: corr {corr:.4}");
    assert!((found.origin[0] - want.origin[0]).abs() < 0.11 && (found.origin[1] - want.origin[1]).abs() < 0.11, "origin {:?} vs pasted {:?}", found.origin, want.origin);
    assert!((found.cell_w - want.cell_w).abs() < 0.05, "cell_w {} vs {}", found.cell_w, want.cell_w);
}

#[test]
fn spread_is_the_largest_change_over_the_judged_numbers() {
    let f = logo();
    let (img, geom) = synth::paint(&f, 4.0, 0.0);
    let a = assess(&img, &geom, &f, Thresholds::default());
    assert_eq!(spread(&a, &a), 0.0);
    let noisy = synth::noise(&img, 0.05, 7);
    let b = assess(&noisy, &geom, &f, Thresholds::default());
    let s = spread(&a, &b);
    assert!(s > 0.0 && s < 0.2, "{s}");
    assert!(s >= (a.correlation - b.correlation).abs() && s >= (a.bleed_max - b.bleed_max).abs() && s >= (a.stray_fraction - b.stray_fraction).abs());
}

// ---------------------------------------------------------------------------------------
// The binary
// ---------------------------------------------------------------------------------------

fn gate(args: &[&str]) -> (i32, String, String) {
    let out = Proc::new(env!("CARGO_BIN_EXE_legibility-gate")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_binary_exits_by_the_report_and_names_the_term_that_failed() {
    let f = logo();
    let dir = scratch("bin");
    let (perfect, geom) = synth::paint(&f, 6.0, 0.0);
    let clean = dir.join("clean.png");
    write_png(&perfect, &clean);
    let soft = dir.join("soft.png");
    write_png(&synth::blur(&perfect, &geom, 0.10), &soft);
    let smeared = dir.join("smeared.png");
    write_png(&synth::blur(&perfect, &geom, 0.30), &smeared);
    let thresholds = dir.join("t.toml");
    write(&thresholds, GOOD_THRESHOLDS);

    // Pass: a clean render, T2's defaults, geometry by search.
    let (code, out, err) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--thresholds", thresholds.to_str().unwrap()]);
    assert_eq!(code, EXIT_PASS, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("legibility-gate: PASS"), "{out}");
    assert!(out.contains("geometry (auto)") && out.contains("pin it with --geom"), "{out}");
    assert!(out.contains("8-bit sRGB"), "the tonemap caveat is on the report: {out}");
    assert!(out.contains("fixture: ") && out.contains("401 lit cells"), "{out}");

    // The same frame, pinned to the geometry the search printed, scores the same.
    let pin = out.lines().find(|l| l.contains("pin it with --geom")).unwrap();
    let pinned = pin.rsplit("--geom ").next().unwrap().trim();
    let (code, out2, _) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--geom", pinned]);
    assert_eq!(code, EXIT_PASS, "{out2}");
    assert!(out2.contains("geometry (explicit)"), "{out2}");
    let summary = |o: &str| o.lines().find(|l| l.starts_with("legibility 81x10")).unwrap().to_string();
    assert_eq!(summary(&out), summary(&out2));

    // Mutation of the threshold: a slight blur scores ~0.999 on correlation; ask for more
    // than it has and the gate fails naming the CORRELATION term and nothing else.
    let strict = dir.join("strict.toml");
    write(&strict, "min_correlation = 0.9995\nmax_bleed = 0.25\nmax_stray = 0.10\nmax_spread = 0.02\n");
    let (code, out, _) = gate(&[soft.to_str().unwrap(), LOGO_PATH, "--thresholds", strict.to_str().unwrap()]);
    assert_eq!(code, EXIT_FAIL, "{out}");
    assert!(out.contains("legibility-gate: FAIL — correlation 0.99") && out.contains("< 1.00"), "{out}");
    assert!(!out.contains("bleed 0.") || !out.contains("FAIL — correlation") || !out.lines().last().unwrap().contains("bleed"), "only the correlation term is named: {out}");

    // A real law-1 failure: a blur past a quarter cell fails on BLEED at the defaults.
    let (code, out, _) = gate(&[smeared.to_str().unwrap(), LOGO_PATH, "--thresholds", thresholds.to_str().unwrap()]);
    assert_eq!(code, EXIT_FAIL, "{out}");
    let last = out.lines().last().unwrap().to_string();
    assert!(last.starts_with("legibility-gate: FAIL — ") && last.contains("bleed 0.") && last.contains("> 0.25"), "{last}");
    assert!(!last.contains("correlation"), "correlation survives a blur that bleed does not: {last}");

    // Determinism: the same frame twice spreads by exactly zero; a noisy twin spreads by
    // more than a tight max_spread and the gate says so.
    let (code, out, _) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--second", clean.to_str().unwrap()]);
    assert_eq!(code, EXIT_PASS, "{out}");
    assert!(out.contains("spread: 0.0000 (<= 0.02 ok)"), "{out}");
    let noisy = dir.join("noisy.png");
    write_png(&synth::noise(&perfect, 0.08, 3), &noisy);
    let tight = dir.join("tight.toml");
    write(&tight, "min_correlation = 0.90\nmax_bleed = 0.25\nmax_stray = 0.10\nmax_spread = 0.0001\n");
    let (code, out, _) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--second", noisy.to_str().unwrap(), "--thresholds", tight.to_str().unwrap()]);
    assert_eq!(code, EXIT_FAIL, "{out}");
    assert!(out.contains("spread") && out.lines().last().unwrap().contains("the two frames do not agree"), "{out}");

    // The cell dump is a cols × rows picture.
    let dump = dir.join("cells.png");
    let (code, _, _) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--dump", dump.to_str().unwrap()]);
    assert_eq!(code, EXIT_PASS);
    let cells = image::open(&dump).unwrap();
    assert_eq!((cells.width(), cells.height()), (81, 10));

    // Could not measure — never a FAIL: a missing frame, a fixture that is not one, a
    // second frame of another size, bad usage.
    let (code, _, err) = gate(&[dir.join("missing.png").to_str().unwrap(), LOGO_PATH]);
    assert_eq!(code, EXIT_USAGE, "{err}");
    assert!(err.contains("reading") && err.contains("missing.png"), "{err}");
    let (code, _, err) = gate(&[clean.to_str().unwrap(), thresholds.to_str().unwrap()]);
    assert_eq!(code, EXIT_USAGE, "{err}");
    assert!(err.contains("fixture"), "{err}");
    let (small, _) = synth::paint(&f, 3.0, 0.0);
    let small_png = dir.join("small.png");
    write_png(&small, &small_png);
    let (code, _, err) = gate(&[clean.to_str().unwrap(), LOGO_PATH, "--second", small_png.to_str().unwrap()]);
    assert_eq!(code, EXIT_USAGE, "{err}");
    assert!(err.contains("--second") && err.contains("but the first frame is"), "{err}");
    let (code, _, err) = gate(&["--geom", "x"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(err.contains("Usage") || err.contains("legibility-gate <frame.png>"), "{err}");
}

#[test]
fn the_binary_reads_the_ring_and_emits_the_producer_input() {
    let f = logo();
    let dir = scratch("bin-ring");
    let ring = dir.join("glyphs.bin");
    let cells = ring_cells(&f);
    write_ring(&ring, &f, &cells, true);
    // The frame is painted from the RING's colours; against the file's one colour the
    // lit-only question cannot even be asked, against the ring it can and is answered.
    let mut grid = Default::default();
    assert!(GlyphRingReader::open_at(&ring).latest_into(&mut grid));
    let rf = fixture_from_grid(&grid, [225; 3]).unwrap();
    let (img, _) = synth::paint(&rf, 5.0, 0.0);
    let png = dir.join("frame.png");
    write_png(&img, &png);
    let (code, out, err) = gate(&[png.to_str().unwrap(), LOGO_PATH, "--ring-file", ring.to_str().unwrap()]);
    assert_eq!(code, EXIT_PASS, "stdout:\n{out}\nstderr:\n{err}");
    assert!(out.contains("colours from the settled ring") && out.contains("401 lit cells"), "{out}");
    assert!(!out.contains("lit-only n/a"), "the ring's gradient gives the lit-only coefficient something to bite on: {out}");
    // A namespace that cannot exist is refused as usage, not scored.
    let (code, _, err) = gate(&[png.to_str().unwrap(), LOGO_PATH, "--ring", "no such ns!"]);
    assert_eq!(code, EXIT_USAGE, "{err}");
    assert!(err.contains("not a usable IPC namespace"), "{err}");

    let (code, out, _) = gate(&["--emit-text", LOGO_PATH]);
    assert_eq!(code, EXIT_PASS);
    assert_eq!(out, emit_text(&f));
}
