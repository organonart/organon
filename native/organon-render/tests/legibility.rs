//! **The legibility harness, end to end and without a GPU** (PBR text T2, organon#217).
//!
//! `src/legibility.rs` makes `doc/pbr_text_engine.md` §9's two laws measurable; this file
//! proves the measurement by running the whole chain — fixture file → synthetic painter →
//! controlled degradation → metric — against inputs whose right answer is known:
//!
//! - a **perfect** render scores ~1 on correlation with zero bleed and zero stray;
//! - a **blur past one cell** fails law 1 (bleed) while the picture is otherwise intact;
//! - a **scramble** fails law 2 (correlation) while the energy budget is identical;
//! - a **brightness gain** changes nothing — §4 puts the phosphor several times above
//!   paper white, and the metric must not care;
//! - **linear light is not optional** — a gamma-wrong render scores lower than a correct
//!   one, and a metric that skipped the decode would not see the difference.
//!
//! Every invariant here was mutation-tested — the harness broken on purpose, the failure
//! message read — and the messages are quoted in the PR that landed this file.
//!
//! ⚠️ Deterministic and CPU-only, so `cargo test -p organon-render` runs all of it. What it
//! cannot say is what a *real* render scores; that leg is T3's, through
//! `legibility::assess_readback_rgba8`.

use organon_render::legibility::{
    assess, assess_readback_rgba8, spill_fraction, synth, Cell, Fixture, GridGeom, Image,
    Thresholds,
};

const LOGO: &str = include_str!("fixtures/omarchy-logo.txt");
const ASYM: &str = include_str!("fixtures/asymmetric.txt");

fn logo() -> Fixture {
    Fixture::parse(LOGO).expect("the Omarchy logo fixture parses")
}
fn asym() -> Fixture {
    Fixture::parse(ASYM).expect("the asymmetric fixture parses")
}

/// A dense fixture — every cell lit, a different colour per cell — built through the
/// parser so the text path is exercised too. The population rule ("blanks are in") has
/// nothing to bite on here, which is what makes it the counterpart to the ~50%-blank logo.
fn dense(cols: usize, rows: usize) -> Fixture {
    let mut text = format!("grid v1\ncols {cols}\nrows {rows}\naspect 2\norder top-down\ndefault #ffffff\n");
    let keys: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().collect();
    assert!(cols * rows <= keys.len());
    for (i, k) in keys.iter().take(cols * rows).enumerate() {
        // A gradient that is not monotone in either axis, so no flip can reproduce it.
        let t = (i * 37 % (cols * rows)) as f32 / (cols * rows - 1) as f32;
        let r = (40.0 + 215.0 * t) as u8;
        let g = (255.0 - 200.0 * t) as u8;
        let b = (80.0 + 120.0 * (1.0 - (2.0 * t - 1.0).abs())) as u8;
        text.push_str(&format!("colour {k} #{r:02x}{g:02x}{b:02x}\n"));
    }
    text.push_str("glyphs\n");
    for _ in 0..rows {
        text.push_str(&format!("|{}|\n", "█".repeat(cols)));
    }
    text.push_str("colours\n");
    for r in 0..rows {
        let row: String = keys[r * cols..(r + 1) * cols].iter().collect();
        text.push_str(&format!("|{row}|\n"));
    }
    Fixture::parse(&text).expect("dense fixture parses")
}

// ---------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------

#[test]
fn the_logo_fixture_reproduces_the_spec_census() {
    let f = logo();
    assert_eq!((f.cols, f.rows), (81, 10), "the source is ragged; its widest row is 81");
    assert_eq!(f.aspect, 2.0);
    let census = f.census();
    // §3: `Counter({'█': 337, ' ': 312, '▄': 32, '▀': 32, '\n': 10})`. The 312 counts the
    // source's EXPLICIT spaces; padding the ragged rows to 81 adds 97 implicit ones.
    assert_eq!(census[&'█'], 337);
    assert_eq!(census[&'▀'], 32);
    assert_eq!(census[&'▄'], 32);
    assert_eq!(census[&' '], 409, "312 explicit + 97 implicit blanks");
    assert_eq!(census.len(), 4, "three glyphs and blank — nothing else");
    assert_eq!(f.lit_count(), 401);
    // Only one colour, so lit-only correlation is undefined on this fixture (see below).
    assert!(f.cells().iter().all(|c| c.fg == [0xc0, 0xca, 0xf5]));
}

#[test]
fn the_logo_fixture_survives_crlf() {
    // The Omarchy checkout is CRLF on Windows; a fixture written from it may be too.
    let crlf = LOGO.replace('\n', "\r\n");
    assert_eq!(Fixture::parse(&crlf).unwrap(), logo());
}

#[test]
fn the_asymmetric_fixture_changes_under_every_flip() {
    let f = asym();
    let flip_v = |f: &Fixture| {
        let mut cells = Vec::new();
        for r in (0..f.rows).rev() {
            cells.extend_from_slice(&f.cells()[r * f.cols..(r + 1) * f.cols]);
        }
        Fixture::from_cells(f.cols, f.rows, f.aspect, cells)
    };
    let flip_h = |f: &Fixture| {
        let mut cells = Vec::new();
        for r in 0..f.rows {
            let row = &f.cells()[r * f.cols..(r + 1) * f.cols];
            cells.extend(row.iter().rev().copied());
        }
        Fixture::from_cells(f.cols, f.rows, f.aspect, cells)
    };
    assert_ne!(flip_v(&f), f, "vertical flip must be visible");
    assert_ne!(flip_h(&f), f, "horizontal flip must be visible");
    assert_ne!(flip_h(&flip_v(&f)), f, "180° rotation must be visible");
}

// ---------------------------------------------------------------------------------------
// The metric against a perfect render
// ---------------------------------------------------------------------------------------

#[test]
fn a_perfect_render_of_the_logo_scores_one() {
    let f = logo();
    let (img, geom) = synth::paint(&f, 6.0, 3.0);
    let r = assess(&img, &geom, &f, Thresholds::default());
    eprintln!("{}", r.summary());
    assert!(r.correlation > 0.9999, "{}", r.summary());
    // One colour, but two coverages (64 half blocks), so the lit-only side has variance —
    // it is asking whether a half block was drawn at half. `None` needs one colour AND one
    // coverage; the unit tests pin that.
    assert!(r.correlation_lit.unwrap() > 0.9999, "{}", r.summary());
    assert_eq!(r.bleed_max, 0.0);
    assert!(r.bleed_at.is_some(), "the logo has blanks beside lit cells");
    assert_eq!(r.stray_fraction, 0.0);
    assert!(r.pass());
    // The half blocks integrate to half a full block, and the expectation says so.
    let full = f.cells().iter().position(|c| c.symbol == '█').unwrap();
    let half = f.cells().iter().position(|c| c.symbol == '▀').unwrap();
    assert!((r.measured[half] / r.measured[full] - 0.5).abs() < 1e-4);
    assert!((r.expected[half] / r.expected[full] - 0.5).abs() < 1e-6);
}

#[test]
fn a_perfect_render_of_the_asymmetric_fixture_scores_one() {
    let f = asym();
    let (img, geom) = synth::paint(&f, 8.0, 4.0);
    let r = assess(&img, &geom, &f, Thresholds::default());
    eprintln!("{}", r.summary());
    assert!(r.correlation > 0.9999, "{}", r.summary());
    let lit = r.correlation_lit.expect("a gradient fixture has a lit-only correlation");
    assert!(lit > 0.9999, "{}", r.summary());
    assert_eq!(r.bleed_max, 0.0);
    assert_eq!(r.stray_fraction, 0.0);
    assert!(r.pass());
}

#[test]
fn orientation_a_flipped_render_of_the_asymmetric_fixture_fails() {
    // Paint the picture upside down and score it against the right-way-up fixture. A
    // harness that mapped rows the wrong way would score THIS one perfectly.
    let f = asym();
    let mut cells = Vec::new();
    for r in (0..f.rows).rev() {
        cells.extend_from_slice(&f.cells()[r * f.cols..(r + 1) * f.cols]);
    }
    let upside_down = Fixture::from_cells(f.cols, f.rows, f.aspect, cells);
    let (img, geom) = synth::paint(&upside_down, 8.0, 4.0);
    let r = assess(&img, &geom, &f, Thresholds::default());
    eprintln!("flipped: {}", r.summary());
    assert!(!r.correlation_ok(), "a vertical flip must not score as legible: {}", r.summary());
    assert!(r.correlation < 0.7, "{}", r.summary());
    assert!(!r.pass());
}

#[test]
fn the_aspect_comes_from_the_fixture_and_a_square_guess_fails() {
    // Painted at 2:1 (from the fixture), scored with a geometry that assumed square cells.
    let f = logo();
    let (img, geom) = synth::paint(&f, 6.0, 3.0);
    assert_eq!(geom.aspect(), 2.0);
    let square = GridGeom { cell_h: geom.cell_w, ..geom };
    let r = assess(&img, &square, &f, Thresholds::default());
    eprintln!("square guess: {}", r.summary());
    assert!(r.correlation < 0.5, "square cells against a 2:1 grid: {}", r.summary());
}

// ---------------------------------------------------------------------------------------
// The two laws, each with its own negative control
// ---------------------------------------------------------------------------------------

#[test]
fn law_one_a_blur_past_one_cell_fails_bleed_and_a_small_one_passes() {
    let f = dense(8, 4);
    let (img, geom) = synth::paint(&f, 8.0, 8.0);
    // Dense grids have no blank neighbours, so bleed reads 0 by construction; law 1 is
    // read from the sparse fixtures. Use both.
    for (name, fixture, cell_w) in [("logo", logo(), 6.0), ("asym", asym(), 8.0)] {
        let (img, geom) = synth::paint(&fixture, cell_w, 3.0 * cell_w);
        let slight = assess(&synth::blur(&img, &geom, 0.05), &geom, &fixture, Thresholds::default());
        let heavy = assess(&synth::blur(&img, &geom, 1.0), &geom, &fixture, Thresholds::default());
        eprintln!("{name} blur 0.05: {}", slight.summary());
        eprintln!("{name} blur 1.00: {}", heavy.summary());
        assert!(slight.pass(), "{name}: a twentieth of a cell of blur is not bleed: {}", slight.summary());
        assert!(!heavy.bleed_ok(), "{name}: a one-cell blur must fail law 1: {}", heavy.summary());
        assert!(heavy.bleed_max > slight.bleed_max);
        assert!(heavy.stray_fraction > slight.stray_fraction);
    }
    // And the dense one, for the record: correlation degrades, bleed cannot register.
    let heavy = assess(&synth::blur(&img, &geom, 1.0), &geom, &f, Thresholds::default());
    eprintln!("dense blur 1.00: {}", heavy.summary());
    assert_eq!(heavy.bleed_at, None, "no blank cell has a lit neighbour on a dense grid");
    assert!(heavy.correlation < 0.9, "{}", heavy.summary());
}

#[test]
fn bleed_is_monotone_in_blur_radius_and_here_is_the_calibration() {
    // Every number the gate thresholds is monotone in the blur it is meant to catch, and
    // the table this prints (`--nocapture`) is what a threshold means in cells of
    // Gaussian halation — the calibration T3 needs to set them on purpose.
    let f = logo();
    let (img, geom) = synth::paint(&f, 6.0, 18.0);
    let sigmas = [0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.5, 1.0];
    let mut prev: Option<(f32, f32, f32)> = None;
    eprintln!("σ (cells)  bleed_max  stray    corr     corr_lit");
    for s in sigmas {
        let r = assess(&synth::blur(&img, &geom, s), &geom, &f, Thresholds::default());
        eprintln!(
            "{s:>8.2}  {:>9.4}  {:>7.4}  {:>7.4}  {:>7.4}  {}",
            r.bleed_max,
            r.stray_fraction,
            r.correlation,
            r.correlation_lit.unwrap_or(f32::NAN),
            if r.pass() { "pass" } else { "FAIL" }
        );
        if let Some((b, st, c)) = prev {
            assert!(r.bleed_max >= b, "bleed must not fall as blur grows (σ={s})");
            assert!(r.stray_fraction >= st, "stray must not fall as blur grows (σ={s})");
            assert!(r.correlation <= c, "correlation must not rise as blur grows (σ={s})");
        }
        prev = Some((r.bleed_max, r.stray_fraction, r.correlation));
    }
}

#[test]
fn law_one_by_isolation_spill_fraction_of_a_single_lit_cell() {
    // One lit cell on an otherwise blank grid: the spec's own phrasing of bleed.
    let mut cells = vec![Cell::BLANK; 5 * 3];
    cells[1 * 5 + 2] = Cell { symbol: '█', fg: [255, 255, 255] };
    let one = Fixture::from_cells(5, 3, 2.0, cells);
    let (img, geom) = synth::paint(&one, 8.0, 16.0);
    assert_eq!(spill_fraction(&img, &geom, 2, 1), 0.0);
    let slight = spill_fraction(&synth::blur(&img, &geom, 0.05), &geom, 2, 1);
    let heavy = spill_fraction(&synth::blur(&img, &geom, 1.0), &geom, 2, 1);
    eprintln!("spill: σ=0.05 → {slight:.4}, σ=1.0 → {heavy:.4}");
    assert!(slight < 0.2, "{slight}");
    // A σ = 1 cell Gaussian keeps ~38% per axis inside ±½ cell → ~15% inside the cell.
    assert!(heavy > 0.75 && heavy < 0.95, "{heavy}");
    // And the grid-wide proxy agrees about which is worse.
    let a = assess(&synth::blur(&img, &geom, 0.05), &geom, &one, Thresholds::default());
    let b = assess(&synth::blur(&img, &geom, 1.0), &geom, &one, Thresholds::default());
    assert!(b.bleed_max > a.bleed_max && !b.bleed_ok() && a.bleed_ok());
}

#[test]
fn law_two_a_scramble_fails_correlation_with_the_same_energy() {
    for (name, fixture, cell_w) in [("logo", logo(), 6.0), ("asym", asym(), 8.0), ("dense", dense(8, 4), 8.0)] {
        let (img, geom) = synth::paint(&fixture.scrambled(3), cell_w, cell_w);
        let r = assess(&img, &geom, &fixture, Thresholds::default());
        eprintln!("{name} scrambled: {}", r.summary());
        assert!(!r.correlation_ok(), "{name}: a scramble must fail law 2: {}", r.summary());
        assert!(r.correlation < 0.5, "{name}: {}", r.summary());
        // Same cells, same light — the total energy is unchanged to float precision.
        let (ok_img, _) = synth::paint(&fixture, cell_w, cell_w);
        let energy = |i: &Image| i.rgb.iter().map(|p| p[0] + p[1] + p[2]).sum::<f32>();
        assert!((energy(&img) - energy(&ok_img)).abs() / energy(&ok_img) < 1e-4);
    }
}

#[test]
fn a_brightness_gain_moves_nothing() {
    // §4: the phosphor at 3–6× paper white. Pearson and both bleed numbers are ratios.
    for (name, fixture, cell_w) in [("logo", logo(), 6.0), ("asym", asym(), 8.0)] {
        let (img, geom) = synth::paint(&fixture, cell_w, cell_w);
        let degraded = synth::noise(&synth::blur(&img, &geom, 0.3), 0.01, 11);
        let base = assess(&degraded, &geom, &fixture, Thresholds::default());
        let hot = assess(&synth::gain(&degraded, 6.0), &geom, &fixture, Thresholds::default());
        eprintln!("{name} 1×: {}", base.summary());
        eprintln!("{name} 6×: {}", hot.summary());
        assert!((base.correlation - hot.correlation).abs() < 1e-5, "{name}");
        assert_eq!(base.correlation_lit.is_some(), hot.correlation_lit.is_some());
        if let (Some(a), Some(b)) = (base.correlation_lit, hot.correlation_lit) {
            assert!((a - b).abs() < 1e-5, "{name}");
        }
        assert!((base.bleed_max - hot.bleed_max).abs() < 1e-5, "{name}");
        assert!((base.stray_fraction - hot.stray_fraction).abs() < 1e-5, "{name}");
        assert_eq!(base.pass(), hot.pass());
        assert!((hot.mean_lit / base.mean_lit - 6.0).abs() < 1e-3, "the light itself did move");
    }
}

#[test]
fn an_eight_bit_readback_clips_the_gain_and_flattens_the_gradient() {
    // The same 6× render through `to_rgba8_srgb` — what a swapchain readback would hand
    // the gate. On the single-colour logo clipping is uniform and harmless; on a gradient
    // it destroys exactly the shape `correlation_lit` measures. So the gate wants the
    // HDR buffer (`from_rgba_f32` / `from_rgba16f`), not the swapchain.
    let f = dense(8, 4);
    let (img, geom) = synth::paint(&f, 8.0, 8.0);
    let hot = synth::gain(&img, 6.0);
    let f32_path = assess(&hot, &geom, &f, Thresholds::default());
    let u8_path = assess(&Image::from_rgba8_srgb(img.width, img.height, &hot.to_rgba8_srgb()), &geom, &f, Thresholds::default());
    eprintln!("6× f32: {}", f32_path.summary());
    eprintln!("6× u8:  {}", u8_path.summary());
    assert!(f32_path.correlation > 0.9999);
    assert!(u8_path.correlation < 0.9, "clipped to white, the gradient is gone: {}", u8_path.summary());
    // Under 1× the byte path is only quantisation away from the float path.
    let u8_sane = assess(&Image::from_rgba8_srgb(img.width, img.height, &img.to_rgba8_srgb()), &geom, &f, Thresholds::default());
    assert!(u8_sane.correlation > 0.9999, "{}", u8_sane.summary());
}

#[test]
fn linear_light_is_not_optional() {
    // Two renders of the gradient fixture. CORRECT: emission = decode(fg), encoded back to
    // bytes — the byte in the image is the byte in the fixture. GAMMA-WRONG: the renderer
    // forgot to decode, emitted fg/255 as if linear, and the display encoded that — every
    // mid-tone comes out too bright, and the gradient's shape is bent (§4).
    let f = dense(8, 4);
    let (correct, geom) = synth::paint(&f, 8.0, 8.0);
    let mut wrong = correct.clone();
    for p in &mut wrong.rgb {
        for c in p.iter_mut() {
            // linear value v was decode(byte); the wrong render emitted byte/255 instead.
            let byte = organon_render::legibility::linear_to_srgb_f(*c);
            *c = byte;
        }
    }
    let r_ok = assess(&correct, &geom, &f, Thresholds::default());
    let r_wrong = assess(&wrong, &geom, &f, Thresholds::default());
    eprintln!("correct:     {}", r_ok.summary());
    eprintln!("gamma-wrong: {}", r_wrong.summary());
    let (ok, bad) = (r_ok.correlation_lit.unwrap(), r_wrong.correlation_lit.unwrap());
    assert!(ok > 0.9999, "{ok}");
    assert!(bad < ok - 0.005, "the gamma-wrong render must score measurably lower: {ok} vs {bad}");
    // The same lesson from the other side: a metric that compared ENCODED values would
    // call the gamma-wrong render perfect. Reproduce that broken metric by hand.
    let encoded_measured: Vec<f32> = r_wrong.measured.iter().map(|m| organon_render::legibility::linear_to_srgb_f(*m)).collect();
    let encoded_expected: Vec<f32> = f.cells().iter().map(|c| organon_render::legibility::luma709([c.fg[0] as f32 / 255.0, c.fg[1] as f32 / 255.0, c.fg[2] as f32 / 255.0])).collect();
    let broken_metric = organon_render::legibility::pearson(&encoded_measured, &encoded_expected);
    eprintln!("the broken (sRGB-space) metric gives the gamma-wrong render {broken_metric:.4}");
    assert!(broken_metric > bad, "an sRGB-space metric ranks the gamma-wrong render higher than the linear one does");
}

#[test]
fn noise_a_little_passes_and_a_lot_fails() {
    let f = logo();
    let (img, geom) = synth::paint(&f, 6.0, 6.0);
    let little = assess(&synth::noise(&img, 0.02, 5), &geom, &f, Thresholds::default());
    let lot = assess(&synth::noise(&img, 2.0, 5), &geom, &f, Thresholds::default());
    eprintln!("noise 0.02: {}", little.summary());
    eprintln!("noise 2.00: {}", lot.summary());
    assert!(little.pass(), "{}", little.summary());
    assert!(!lot.pass(), "{}", lot.summary());
    assert!(lot.stray_fraction > little.stray_fraction);
}

// ---------------------------------------------------------------------------------------
// The population rule, sparse vs dense
// ---------------------------------------------------------------------------------------

#[test]
fn a_uniform_flood_is_invisible_to_correlation_and_caught_by_stray() {
    // Every black pixel — blank cells AND the dark halves of half blocks — raised to 40%
    // of the lit level: a lit backplane, a fog. Measured becomes `F + coverage·(L − F)`,
    // which is AFFINE in the expected `coverage·L`, and Pearson cannot see an affine map.
    // ⚠️ So `correlation` reads 1.0 on a picture that is plainly fogged. That is not a
    // defect to fix in the correlation; it is the reason the report carries
    // `stray_fraction` and `bleed_max` beside it, and why `pass()` needs all three.
    let f = logo();
    let (mut img, geom) = synth::paint(&f, 6.0, 0.0);
    let lit_level = img.rgb.iter().map(|p| p[1]).fold(0.0f32, f32::max);
    for p in &mut img.rgb {
        if p[0] + p[1] + p[2] == 0.0 {
            *p = [lit_level * 0.4; 3];
        }
    }
    let r = assess(&img, &geom, &f, Thresholds::default());
    eprintln!("fogged: {}", r.summary());
    assert!(r.correlation > 0.9999, "affine → Pearson 1: {}", r.summary());
    assert!(!r.stray_ok() && !r.bleed_ok(), "{}", r.summary());
    assert!(!r.pass());
}

#[test]
fn blanks_are_in_the_population_and_lit_blanks_lower_the_score() {
    // Only the BLANK cells are lit, each to a level that varies across the grid, and the
    // lit cells are untouched. Lit-only correlation stays 1.0 — the text itself is
    // perfect — while the all-cells number drops, because the fixture said those cells
    // were dark and they are not. Excluding blanks from the population would make the
    // two numbers equal and the gate blind to this.
    let f = logo();
    let (mut img, geom) = synth::paint(&f, 6.0, 0.0);
    for r in 0..f.rows {
        for c in 0..f.cols {
            if f.cell(c, r).is_lit() {
                continue;
            }
            let [x0, y0, x1, y1] = geom.cell_rect(c, r);
            let level = 0.6 * (c as f32 / f.cols as f32);
            for y in y0 as usize..y1 as usize {
                for x in x0 as usize..x1 as usize {
                    img.rgb[y * img.width + x] = [level; 3];
                }
            }
        }
    }
    let r = assess(&img, &geom, &f, Thresholds::default());
    eprintln!("lit blanks: {}", r.summary());
    assert!(r.correlation_lit.unwrap() > 0.9999, "the text is untouched: {}", r.summary());
    assert!(r.correlation < 0.9, "the blanks are in the population: {}", r.summary());
    assert!(!r.pass());
    // On a dense grid there are no blanks, and all-cells equals lit-only exactly.
    let d = dense(8, 4);
    let (dimg, dgeom) = synth::paint(&d, 8.0, 8.0);
    let dr = assess(&synth::noise(&dimg, 0.05, 9), &dgeom, &d, Thresholds::default());
    assert_eq!(Some(dr.correlation), dr.correlation_lit);
    assert_eq!(dr.bleed_at, None);
}

// ---------------------------------------------------------------------------------------
// The GPU-facing entry point
// ---------------------------------------------------------------------------------------

#[test]
fn the_readback_entry_point_needs_no_geometry() {
    // A frame that the grid fills, as a front-on gate render would: paint, encode to the
    // bytes a `Rgba8UnormSrgb` readback yields, and hand only (w, h, bytes, fixture) over.
    let f = asym();
    let (img, geom) = synth::paint(&f, 10.0, 0.0);
    assert_eq!(geom.origin, [0.0, 0.0]);
    let bytes = img.to_rgba8_srgb();
    let r = assess_readback_rgba8(img.width, img.height, &bytes, &f, Thresholds::default());
    eprintln!("readback: {}", r.summary());
    assert!(r.correlation > 0.9999, "{}", r.summary());
    assert!(r.pass());
    // Pillarboxed — the grid fills the height and sits centred in a wider frame — the
    // fit still finds it. ⚠️ It cannot find a grid padded on BOTH axes: `fit` assumes the
    // grid touches the frame along at least one axis (a first draft of this test padded
    // both and scored 0.59). A gate render that does not fill its frame must hand
    // `assess` its own `GridGeom`.
    let (w, h) = (img.width + 40, img.height);
    let mut padded = vec![0u8; w * h * 4];
    for y in 0..img.height {
        let src = &bytes[y * img.width * 4..(y + 1) * img.width * 4];
        let dst = (y * w + 20) * 4;
        padded[dst..dst + src.len()].copy_from_slice(src);
    }
    let r = assess_readback_rgba8(w, h, &padded, &f, Thresholds::default());
    eprintln!("pillarboxed: {}", r.summary());
    assert!(r.correlation > 0.9999, "centred fit: {}", r.summary());
    assert!(r.pass());
}
