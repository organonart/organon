//! # The legibility harness — `doc/pbr_text_engine.md` §9, as a number
//!
//! **PBR text Tier 2 (organon#217).** A glyph grid stays readable for two reasons that have
//! nothing to do with any tile's silhouette, and §9 states them as laws:
//!
//! > 1. **The cell's energy stays in the cell.** Inter-cell bleed is the only thing that
//! >    actually destroys text.
//! > 2. **The cell's apparent brightness tracks the effect's value.** Whatever happens
//! >    inside a cell, its integrated luminance must correlate with what TTE said it was.
//!
//! This module makes both measurable, on the CPU, from any image: a GPU readback, a
//! synthetic painting, a PNG off disk. Downsample the image to the cell grid, compare the
//! per-cell luminance with what the fixture says each cell should be, and *"is this preset
//! still readable"* stops being taste. It is what T3's preset gate calls, and what T1's
//! first real render is scored with — and it has **no GPU in it**, which is why it is one
//! of the rare things in this repository that is fully verified by `cargo test`.
//!
//! ## The pieces
//!
//! | thing | what it is |
//! |---|---|
//! | [`Fixture`] | the *source of truth*: a cell grid with a symbol and an sRGB foreground per cell, parsed from a small hand-readable text file (`tests/fixtures/*.txt`; grammar below) |
//! | [`Image`] | the *render under test*, held in **linear light** whatever it arrived as — sRGB bytes, `f32`, or `Rgba16Float` halves |
//! | [`GridGeom`] | where the grid sits in the image: the pixel origin of cell `(0, 0)` and the cell size, with the **2:1 aspect** carried from the fixture rather than assumed |
//! | [`downsample`] | one box filter per cell, area-weighted at fractional pixel boundaries, luma per Rec. 709 — the per-cell measurement |
//! | [`assess`] / [`assess_readback_rgba8`] | the entry points: measurement → [`Report`], judged against a [`Thresholds`] that is a **parameter**, never a constant |
//! | [`synth`] | a tiny CPU painter and four controllable degradations, so the metric can be tested against *known* inputs without an adapter |
//!
//! ## The metric, and its units
//!
//! Everything is in **linear light**, and this is the decision the number rests on. A TTE
//! colour is an sRGB-encoded byte triple (§4), so the fixture's expected luma is
//! `luma709(srgb_to_linear(fg))`, and an image that arrives as sRGB bytes is decoded per
//! pixel *before* the box filter — averaging encoded values and decoding the mean is not
//! the same operation, and the difference is largest exactly at a glyph edge. A metric
//! computed in sRGB would rank a gamma-wrong render above a correct one; the test
//! `linear_light_is_not_optional` in `tests/legibility.rs` pins that the decode moves the
//! score in the direction predicted.
//!
//! - **Expected luma per cell** is the fg colour's linear luma **times the glyph's
//!   coverage** — `█` is 1, `▀`/`▄`/`▌`/`▐` are ½, `░▒▓` are ¼/½/¾, the eighth blocks are
//!   their fraction, a blank is 0, and any other symbol is treated as a full cell (the
//!   approximation is named in [`glyph_shape`]). §9 says "what TTE said that cell was";
//!   what TTE said about a `▀` cell is *half of that colour*, and a renderer that draws a
//!   half-height tile emits half the light, so the expectation has to say so or a perfect
//!   render of the Omarchy logo (64 half blocks) could never score 1.
//! - **Measured luma per cell** is the mean linear luma over the cell's pixel footprint.
//!   Its scale is whatever the image carries — `1.0` = SDR white for an `f32` or half-float
//!   readback, the decoded byte for sRGB input — and the metric never depends on it.
//! - **`correlation`** is the Pearson coefficient between measured and expected over
//!   **every cell, blank cells included**. That is a choice, and the alternative was
//!   rejected on purpose: excluding blanks would let a render that floods the empty cells
//!   with light score perfectly, and law 2 says the empty cells were *told* to be dark. It
//!   costs something on a sparse grid — the Omarchy logo is 409 blanks to 401 lit cells, so
//!   the number is roughly half "did the dark stay dark" — which is why the report also
//!   carries **`correlation_lit`**, the same coefficient over lit cells only: it asks
//!   whether the *gradient's shape* survived inside the text, independent of the blanks.
//!   When every lit cell expects the same luma — one colour **and** one coverage — the
//!   expected side has no variance and it is `None`. The logo is one colour but two
//!   coverages (64 half blocks), so there it asks only whether a half block was drawn at
//!   half; `asymmetric.txt` carries a gradient so the question has more to bite on.
//!   Pearson is invariant to a positive gain, so a phosphor at 6× paper white scores the
//!   same as one at 1× — which §4 requires, since gain is a *look* parameter.
//!   ⚠️ **It is equally invariant to an offset, and that is a blind spot to know about**:
//!   a uniform fog over the whole frame — every dark pixel raised by the same amount,
//!   inside half blocks as well as in blanks — makes measured an *affine* function of
//!   expected, and `correlation` reads exactly 1.0 on a picture that is plainly fogged
//!   (`a_uniform_flood_is_invisible_to_correlation_and_caught_by_stray` pins it). That
//!   is not a defect of the correlation; it is why the report carries the two law-1
//!   numbers beside it and why `pass()` needs all three.
//! - **`bleed_max`** is law 1 read off the grid: for every blank cell with at least one
//!   lit 8-neighbour, its measured luma divided by the mean measured luma of those
//!   neighbours, and the maximum over the grid — *the fraction of a neighbour's light that
//!   landed in a cell that was told to be dark*. Dimensionless, gain-invariant, and it is
//!   exactly the quantity that fills in the gap between two strokes. 📌 The spec's
//!   phrasing — "the fraction of each **lit** cell's energy that lands outside its own
//!   footprint" — is not measurable from a multi-cell image, because a rendered pixel does
//!   not say which cell it came from. It *is* measurable by isolation, and
//!   [`spill_fraction`] does that for a one-cell image; the synthetic self-test uses it to
//!   calibrate what a blur of *r* cells does to `bleed_max`.
//! - **`stray_fraction`** is the share of the grid's total energy that landed in blank
//!   cells — the global form of law 1, which also catches a lit backplane or a fog that
//!   `bleed_max`, being local, would miss.
//!
//! ## The fixture grammar
//!
//! Plain text, one directive per line, `#` comments, CRLF tolerated (the Omarchy checkout
//! on Windows is CRLF). Chosen over JSON because a person has to be able to *see* the
//! grid, and a JSON string of block characters is not a picture:
//!
//! ```text
//! grid v1                 # first non-comment line, always
//! cols 5                  # cell columns
//! rows 3                  # cell rows
//! aspect 2                # cell height ÷ width — TTE's cell is 2:1 (§7)
//! order top-down          # or bottom-up: which end of the picture the FIRST fenced line is
//! default #ffffff         # fg for any cell the `colours` block leaves blank
//! colour r #ff3000        # single-character colour keys (`color` also accepted)
//! glyphs                  # exactly `rows` lines, each `|` + `cols` symbols + `|`
//! |█▀   |
//! |█  ▒ |
//! |█▄▄█ |
//! colours                 # optional; same shape; ` ` or `.` means `default`
//! |ro   |
//! |y  b |
//! |gobr |
//! ```
//!
//! ⚠️ **The fences are load-bearing.** The Omarchy logo's source rows are ragged (the first
//! is 20 characters, the widest 81), and a format that let trailing blanks be implicit
//! would make the grid's width a matter of which editor last saved it. Every row must be
//! exactly `cols` symbols between its fences or the parse fails, naming the line.
//!
//! ⚠️ **Orientation is written, never assumed.** In memory row 0 is always the **top** of
//! the picture, which is also image row 0 for a wgpu readback, so [`GridGeom`] needs no
//! flip. `order bottom-up` exists for a producer walking a ttfx grid, whose `Coord` is
//! 1-based from the bottom-left (§7): write the rows in the order you walk them and say so.
//! The asymmetric fixture is what proves the flip — the logo is too symmetric to notice.
//!
//! ## What is verified here, and what is not
//!
//! Everything in this file is deterministic CPU code, and `tests/legibility.rs` runs the
//! whole chain — fixture → painter → degradation → metric — against known answers, with
//! every invariant mutation-tested. **What no test here can say is what a real render
//! scores.** That leg needs an adapter and is T3's; its entry point is
//! [`assess_readback_rgba8`], and the [`GridGeom`] it will need is axis-aligned — a
//! front-on, orthographic gate render, or a homography this module does not yet have.

use std::collections::BTreeMap;
use std::fmt;

/// TTE's cell aspect — height ÷ width. Every ring, circle and spiral in the effect set is
/// authored for a cell twice as tall as it is wide (§7). Fixtures carry their own; this is
/// the value a producer should write.
pub const ASPECT_TTE: f32 = 2.0;

// ---------------------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------------------

/// sRGB byte → linear, the IEC 61966-2-1 piecewise decode.
pub fn srgb_to_linear(v: u8) -> f32 {
    srgb_to_linear_f(v as f32 / 255.0)
}

/// sRGB `[0, 1]` → linear.
pub fn srgb_to_linear_f(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear → sRGB `[0, 1]`, clamped. The painter uses it to produce byte images that a
/// display would show as the fixture's colours.
pub fn linear_to_srgb_f(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Rec. 709 luma of a **linear** RGB triple. Same coefficients `gi.rs` and `theme.rs`
/// already use for the same purpose.
pub fn luma709(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

// ---------------------------------------------------------------------------------------
// Glyphs
// ---------------------------------------------------------------------------------------

/// The footprint a symbol lights inside its cell, in cell-relative coordinates with
/// `(0, 0)` the cell's **top-left** and `(1, 1)` its bottom-right, plus an intensity for
/// the dithered shade blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// `1.0` for solid blocks; `¼ ½ ¾` for `░ ▒ ▓`, whose stipple integrates to that
    /// fraction of the colour over the cell.
    pub intensity: f32,
}

impl Glyph {
    /// The fraction of the cell's light this glyph carries — area × intensity.
    pub fn coverage(&self) -> f32 {
        (self.x1 - self.x0) * (self.y1 - self.y0) * self.intensity
    }
}

/// The sub-cell rectangle a symbol lights, or `None` for a blank.
///
/// §3 measured that the content this harness exists for is three glyphs — `█ ▀ ▄` — and
/// that what the 36 effects substitute in is overwhelmingly the block family. Those are
/// modelled exactly. ⚠️ **Anything else is treated as a full cell.** A letterform is not
/// a full cell, so a fixture full of ASCII will carry an expectation that is too bright
/// per lit cell; the *correlation* survives that (it is uniform across cells) but the
/// bleed ratio's denominator is inflated. T7 (real letterforms) is where a per-glyph
/// coverage table would come from.
pub fn glyph_shape(symbol: char) -> Option<Glyph> {
    let full = |i: f32| Glyph { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0, intensity: i };
    Some(match symbol {
        ' ' | '\u{a0}' => return None,
        '█' => full(1.0),
        '░' => full(0.25),
        '▒' => full(0.5),
        '▓' => full(0.75),
        // Upper half / lower half.
        '▀' => Glyph { x0: 0.0, y0: 0.0, x1: 1.0, y1: 0.5, intensity: 1.0 },
        '▄' => Glyph { x0: 0.0, y0: 0.5, x1: 1.0, y1: 1.0, intensity: 1.0 },
        // Left half / right half.
        '▌' => Glyph { x0: 0.0, y0: 0.0, x1: 0.5, y1: 1.0, intensity: 1.0 },
        '▐' => Glyph { x0: 0.5, y0: 0.0, x1: 1.0, y1: 1.0, intensity: 1.0 },
        // Lower eighths ▁▂▃▄▅▆▇ (U+2581..U+2587); ▄ handled above, kept in the run.
        '▁' | '▂' | '▃' | '▅' | '▆' | '▇' => {
            let k = (symbol as u32 - '▁' as u32 + 1) as f32 / 8.0;
            Glyph { x0: 0.0, y0: 1.0 - k, x1: 1.0, y1: 1.0, intensity: 1.0 }
        }
        // Left eighths ▏▎▍▌▋▊▉ (U+258F down to U+2589); ▌ above.
        '▏' | '▎' | '▍' | '▋' | '▊' | '▉' => {
            let k = ('▏' as u32 - symbol as u32 + 1) as f32 / 8.0;
            Glyph { x0: 0.0, y0: 0.0, x1: k, y1: 1.0, intensity: 1.0 }
        }
        // Quadrants.
        '▖' => Glyph { x0: 0.0, y0: 0.5, x1: 0.5, y1: 1.0, intensity: 1.0 },
        '▗' => Glyph { x0: 0.5, y0: 0.5, x1: 1.0, y1: 1.0, intensity: 1.0 },
        '▘' => Glyph { x0: 0.0, y0: 0.0, x1: 0.5, y1: 0.5, intensity: 1.0 },
        '▝' => Glyph { x0: 0.5, y0: 0.0, x1: 1.0, y1: 0.5, intensity: 1.0 },
        _ => full(1.0),
    })
}

// ---------------------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------------------

/// One cell of a fixture: what TTE said was there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub symbol: char,
    /// sRGB-encoded foreground, as TTE emits it.
    pub fg: [u8; 3],
}

impl Cell {
    pub const BLANK: Cell = Cell { symbol: ' ', fg: [0, 0, 0] };

    pub fn is_lit(&self) -> bool {
        glyph_shape(self.symbol).is_some()
    }

    /// The linear luma this cell should integrate to: coverage × luma(decode(fg)).
    pub fn expected_luma(&self) -> f32 {
        match glyph_shape(self.symbol) {
            None => 0.0,
            Some(g) => {
                g.coverage()
                    * luma709([
                        srgb_to_linear(self.fg[0]),
                        srgb_to_linear(self.fg[1]),
                        srgb_to_linear(self.fg[2]),
                    ])
            }
        }
    }
}

/// Which end of the picture the first fenced line in a fixture file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowOrder {
    /// The first line is the top of the picture — how a person reads it.
    TopDown,
    /// The first line is the bottom — TTE's native row order (`Coord` row 1 is the
    /// bottom row), so a producer can write rows in the order it walks them.
    BottomUp,
}

/// A cell grid: the ground truth a render is scored against.
///
/// Row-major, and **row 0 is the top of the picture** regardless of the file's `order`.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixture {
    pub cols: usize,
    pub rows: usize,
    /// Cell height ÷ width. [`ASPECT_TTE`] for anything from TTE.
    pub aspect: f32,
    cells: Vec<Cell>,
}

/// A parse failure, with the 1-based line it was found on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fixture line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for ParseError {}

fn parse_hex_colour(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([ch(0)?, ch(2)?, ch(4)?])
}

impl Fixture {
    /// Build a fixture from cells given in row-major order with row 0 at the top.
    pub fn from_cells(cols: usize, rows: usize, aspect: f32, cells: Vec<Cell>) -> Fixture {
        assert_eq!(cells.len(), cols * rows, "cell count must be cols × rows");
        assert!(aspect > 0.0, "aspect must be positive");
        Fixture { cols, rows, aspect, cells }
    }

    /// Parse the text format described in the module doc.
    pub fn parse(text: &str) -> Result<Fixture, ParseError> {
        let err = |line: usize, msg: String| Err(ParseError { line, msg });
        let mut cols: Option<usize> = None;
        let mut rows: Option<usize> = None;
        let mut aspect: Option<f32> = None;
        let mut order: Option<RowOrder> = None;
        let mut default: Option<[u8; 3]> = None;
        let mut palette: BTreeMap<char, [u8; 3]> = BTreeMap::new();
        let mut glyph_lines: Vec<(usize, Vec<char>)> = Vec::new();
        let mut colour_lines: Vec<(usize, Vec<char>)> = Vec::new();

        #[derive(PartialEq)]
        enum Block {
            Header,
            Glyphs,
            Colours,
        }
        let mut block = Block::Header;
        let mut seen_magic = false;

        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            let line = raw.trim_end_matches('\r');
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // A fenced row belongs to whichever block is open.
            if trimmed.starts_with('|') {
                let inner = match trimmed.strip_prefix('|').and_then(|s| s.strip_suffix('|')) {
                    Some(inner) => inner,
                    None => return err(line_no, "a fenced row must end with `|`".into()),
                };
                let chars: Vec<char> = inner.chars().collect();
                match block {
                    Block::Glyphs => glyph_lines.push((line_no, chars)),
                    Block::Colours => colour_lines.push((line_no, chars)),
                    Block::Header => {
                        return err(line_no, "a fenced row before `glyphs` or `colours`".into())
                    }
                }
                continue;
            }
            let mut words = trimmed.split_whitespace();
            let key = words.next().unwrap_or("");
            let rest: Vec<&str> = words.collect();
            if !seen_magic {
                if key == "grid" && rest == ["v1"] {
                    seen_magic = true;
                    continue;
                }
                return err(line_no, "expected `grid v1` as the first directive".to_string());
            }
            match key {
                "cols" | "rows" => {
                    let n: usize = rest
                        .first()
                        .and_then(|s| s.parse().ok())
                        .filter(|n: &usize| *n > 0)
                        .ok_or_else(|| ParseError { line: line_no, msg: format!("`{key}` needs a positive integer") })?;
                    if key == "cols" { cols = Some(n) } else { rows = Some(n) }
                    block = Block::Header;
                }
                "aspect" => {
                    let a: f32 = rest
                        .first()
                        .and_then(|s| s.parse().ok())
                        .filter(|a: &f32| *a > 0.0 && a.is_finite())
                        .ok_or_else(|| ParseError { line: line_no, msg: "`aspect` needs a positive number (height ÷ width)".into() })?;
                    aspect = Some(a);
                    block = Block::Header;
                }
                "order" => {
                    order = Some(match rest.first().copied() {
                        Some("top-down") => RowOrder::TopDown,
                        Some("bottom-up") => RowOrder::BottomUp,
                        _ => return err(line_no, "`order` must be `top-down` or `bottom-up`".into()),
                    });
                    block = Block::Header;
                }
                "default" => {
                    default = Some(
                        rest.first()
                            .and_then(|s| parse_hex_colour(s))
                            .ok_or_else(|| ParseError { line: line_no, msg: "`default` needs `#rrggbb`".into() })?,
                    );
                    block = Block::Header;
                }
                "colour" | "color" => {
                    let (k, v) = match rest.as_slice() {
                        [k, v] => (*k, *v),
                        _ => return err(line_no, "`colour` needs a one-character key and `#rrggbb`".into()),
                    };
                    let mut kc = k.chars();
                    let key_char = match (kc.next(), kc.next()) {
                        (Some(c), None) if c != ' ' && c != '.' => c,
                        _ => return err(line_no, "a colour key is exactly one character, not ` ` or `.`".into()),
                    };
                    let rgb = parse_hex_colour(v)
                        .ok_or_else(|| ParseError { line: line_no, msg: "`colour` value must be `#rrggbb`".into() })?;
                    palette.insert(key_char, rgb);
                    block = Block::Header;
                }
                "glyphs" => block = Block::Glyphs,
                "colours" | "colors" => block = Block::Colours,
                other => return err(line_no, format!("unknown directive `{other}`")),
            }
        }

        if !seen_magic {
            return err(0, "empty fixture: no `grid v1` line".into());
        }
        let cols = cols.ok_or_else(|| ParseError { line: 0, msg: "missing `cols`".into() })?;
        let rows = rows.ok_or_else(|| ParseError { line: 0, msg: "missing `rows`".into() })?;
        let aspect = aspect.ok_or_else(|| ParseError { line: 0, msg: "missing `aspect` (TTE's is 2)".into() })?;
        let order = order.ok_or_else(|| ParseError { line: 0, msg: "missing `order` (top-down or bottom-up) — orientation is written, never assumed".into() })?;
        let default = default.ok_or_else(|| ParseError { line: 0, msg: "missing `default` colour".into() })?;

        let check_rows = |lines: &[(usize, Vec<char>)], what: &str| -> Result<(), ParseError> {
            if lines.len() != rows {
                return Err(ParseError {
                    line: lines.last().map(|l| l.0).unwrap_or(0),
                    msg: format!("`{what}` has {} fenced rows, `rows` says {rows}", lines.len()),
                });
            }
            for (line_no, chars) in lines {
                if chars.len() != cols {
                    return Err(ParseError {
                        line: *line_no,
                        msg: format!("`{what}` row has {} symbols between its fences, `cols` says {cols}", chars.len()),
                    });
                }
            }
            Ok(())
        };
        if glyph_lines.is_empty() {
            return err(0, "missing `glyphs` block".into());
        }
        check_rows(&glyph_lines, "glyphs")?;
        if !colour_lines.is_empty() {
            check_rows(&colour_lines, "colours")?;
        }

        let mut cells = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let symbol = glyph_lines[r].1[c];
                let fg = match colour_lines.get(r).map(|l| l.1[c]) {
                    None | Some(' ') | Some('.') => default,
                    Some(k) => match palette.get(&k) {
                        Some(rgb) => *rgb,
                        None => {
                            return err(colour_lines[r].0, format!("colour key `{k}` is not declared"))
                        }
                    },
                };
                cells.push(Cell { symbol, fg });
            }
        }
        if order == RowOrder::BottomUp {
            let mut flipped = Vec::with_capacity(cells.len());
            for r in (0..rows).rev() {
                flipped.extend_from_slice(&cells[r * cols..(r + 1) * cols]);
            }
            cells = flipped;
        }
        Ok(Fixture { cols, rows, aspect, cells })
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// `(col, row)`, row 0 at the top.
    pub fn cell(&self, col: usize, row: usize) -> &Cell {
        &self.cells[row * self.cols + col]
    }

    /// Per-cell expected linear luma, row-major.
    pub fn expected_luma(&self) -> Vec<f32> {
        self.cells.iter().map(Cell::expected_luma).collect()
    }

    pub fn lit_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_lit()).count()
    }

    /// Symbol counts — the §3 census, reproducible from the fixture.
    pub fn census(&self) -> BTreeMap<char, usize> {
        let mut m = BTreeMap::new();
        for c in &self.cells {
            *m.entry(c.symbol).or_insert(0) += 1;
        }
        m
    }

    /// The same cells in a different order — every symbol and colour kept, every position
    /// permuted (Fisher–Yates over a seeded xorshift). Law 2's negative control: the
    /// energy budget is identical, the picture is gone.
    pub fn scrambled(&self, seed: u64) -> Fixture {
        let mut cells = self.cells.clone();
        let mut rng = XorShift::new(seed);
        for i in (1..cells.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            cells.swap(i, j);
        }
        Fixture { cols: self.cols, rows: self.rows, aspect: self.aspect, cells }
    }
}

/// Deterministic, dependency-free PRNG for the negative controls. Not for anything that
/// needs to be random; for anything that needs to be *the same every run*.
#[derive(Clone, Debug)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        // Zero is xorshift's one fixed point; nudge it off.
        XorShift(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform in `[-1, 1)`.
    pub fn signed_unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 23) as f32 * 2.0 - 1.0
    }
}

// ---------------------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------------------

/// A render under test, in **linear light**, row 0 at the top (a wgpu readback's order).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Row-major linear RGB.
    pub rgb: Vec<[f32; 3]>,
}

impl Image {
    pub fn black(width: usize, height: usize) -> Image {
        Image { width, height, rgb: vec![[0.0; 3]; width * height] }
    }

    /// From tightly packed sRGB-encoded RGBA8 — a readback of an `Rgba8UnormSrgb` target,
    /// a PNG's pixels. Alpha is ignored. Decoded per pixel, before anything is averaged.
    pub fn from_rgba8_srgb(width: usize, height: usize, bytes: &[u8]) -> Image {
        assert_eq!(bytes.len(), width * height * 4, "RGBA8 buffer is not width × height × 4");
        let rgb = bytes
            .chunks_exact(4)
            .map(|p| [srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2])])
            .collect();
        Image { width, height, rgb }
    }

    /// From tightly packed **linear** RGBA `f32` — an HDR scene-buffer readback. Alpha is
    /// ignored; values above 1 are kept (this is the path a 6× phosphor must take — an
    /// 8-bit readback clips it, and `tests/legibility.rs` shows what that costs).
    pub fn from_rgba_f32(width: usize, height: usize, px: &[f32]) -> Image {
        assert_eq!(px.len(), width * height * 4, "RGBA f32 buffer is not width × height × 4");
        let rgb = px.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
        Image { width, height, rgb }
    }

    /// From tightly packed **linear** `Rgba16Float` bits — the EDR swapchain / HDR buffer
    /// format the visual renders into. Uses the `half` crate this crate already carries.
    pub fn from_rgba16f(width: usize, height: usize, px: &[u16]) -> Image {
        assert_eq!(px.len(), width * height * 4, "RGBA16F buffer is not width × height × 4");
        let f = |b: u16| half::f16::from_bits(b).to_f32();
        let rgb = px.chunks_exact(4).map(|p| [f(p[0]), f(p[1]), f(p[2])]).collect();
        Image { width, height, rgb }
    }

    /// Encode to tightly packed sRGB RGBA8, alpha 255, **clipping** anything above 1. The
    /// painter's route to an 8-bit test image; what a swapchain readback would have done.
    pub fn to_rgba8_srgb(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgb.len() * 4);
        for p in &self.rgb {
            for c in p {
                out.push((linear_to_srgb_f(*c) * 255.0).round().clamp(0.0, 255.0) as u8);
            }
            out.push(255);
        }
        out
    }

    #[inline]
    pub fn luma(&self, x: usize, y: usize) -> f32 {
        luma709(self.rgb[y * self.width + x])
    }
}

impl From<&image::RgbaImage> for Image {
    fn from(img: &image::RgbaImage) -> Image {
        Image::from_rgba8_srgb(img.width() as usize, img.height() as usize, img.as_raw())
    }
}

// ---------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------

/// Where the cell grid sits in the image, in pixels. Axis-aligned: cell `(c, r)` covers
/// `[origin.x + c·cell_w, +cell_w) × [origin.y + r·cell_h, +cell_h)`, row 0 at the **top**.
///
/// ⚠️ Axis-aligned is a real limit: a preset that tilts the camera (§11's `bottled`) has
/// to be gated from a front-on render, or this needs a homography. Not built; say so
/// rather than approximate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridGeom {
    /// Pixel position of the top-left corner of cell `(0, 0)`. Fractional is fine.
    pub origin: [f32; 2],
    pub cell_w: f32,
    pub cell_h: f32,
}

impl GridGeom {
    /// A grid of `cell_w`-wide cells at the fixture's aspect, at `origin`.
    pub fn at(origin: [f32; 2], cell_w: f32, aspect: f32) -> GridGeom {
        GridGeom { origin, cell_w, cell_h: cell_w * aspect }
    }

    /// The largest grid at the fixture's aspect that fits an image of this size, centred.
    /// What [`assess_readback_rgba8`] uses; a gate render whose grid **touches the frame
    /// along at least one axis** (letterboxed or pillarboxed, never both) can call the
    /// entry point with no geometry of its own. ⚠️ A grid padded on both axes cannot be
    /// found this way — there is no information in a black border about how much of it
    /// is border — and scores as misaligned; hand [`assess`] the real geometry instead.
    pub fn fit(width: usize, height: usize, fixture: &Fixture) -> GridGeom {
        let by_w = width as f32 / fixture.cols as f32;
        let by_h = height as f32 / (fixture.rows as f32 * fixture.aspect);
        let cell_w = by_w.min(by_h);
        let cell_h = cell_w * fixture.aspect;
        let gw = cell_w * fixture.cols as f32;
        let gh = cell_h * fixture.rows as f32;
        GridGeom {
            origin: [(width as f32 - gw) * 0.5, (height as f32 - gh) * 0.5],
            cell_w,
            cell_h,
        }
    }

    pub fn aspect(&self) -> f32 {
        self.cell_h / self.cell_w
    }

    /// Pixel rectangle `[x0, x1) × [y0, y1)` of cell `(col, row)`.
    pub fn cell_rect(&self, col: usize, row: usize) -> [f32; 4] {
        let x0 = self.origin[0] + col as f32 * self.cell_w;
        let y0 = self.origin[1] + row as f32 * self.cell_h;
        [x0, y0, x0 + self.cell_w, y0 + self.cell_h]
    }
}

// ---------------------------------------------------------------------------------------
// The measurement
// ---------------------------------------------------------------------------------------

/// Overlap length of `[a0, a1)` and `[b0, b1)`, never negative.
#[inline]
fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Box-filter the image to the cell grid: the **mean linear luma** of each cell's pixel
/// footprint, row-major, row 0 at the top. Area-weighted, so a pixel straddling two cells
/// (any non-integer cell size, any fractional origin) is split by its overlap rather than
/// assigned whole. Pixels outside the grid contribute to nothing; a cell hanging off the
/// image edge integrates only what is there, and reads dark for it.
pub fn downsample(image: &Image, geom: &GridGeom, cols: usize, rows: usize) -> Vec<f32> {
    let mut sum = vec![0.0f64; cols * rows];
    let [ox, oy] = geom.origin;
    let (cw, ch) = (geom.cell_w, geom.cell_h);
    assert!(cw > 0.0 && ch > 0.0, "cell size must be positive");
    let gx1 = ox + cw * cols as f32;
    let gy1 = oy + ch * rows as f32;
    for py in 0..image.height {
        let (y0, y1) = (py as f32, py as f32 + 1.0);
        if y1 <= oy || y0 >= gy1 {
            continue;
        }
        let r0 = (((y0 - oy) / ch).floor().max(0.0)) as usize;
        let r1 = (((y1 - oy) / ch).floor().max(0.0) as usize).min(rows - 1);
        for px in 0..image.width {
            let (x0, x1) = (px as f32, px as f32 + 1.0);
            if x1 <= ox || x0 >= gx1 {
                continue;
            }
            let c0 = (((x0 - ox) / cw).floor().max(0.0)) as usize;
            let c1 = (((x1 - ox) / cw).floor().max(0.0) as usize).min(cols - 1);
            let l = image.luma(px, py) as f64;
            for r in r0..=r1 {
                let cy0 = oy + r as f32 * ch;
                let wy = overlap(y0, y1, cy0, cy0 + ch);
                if wy <= 0.0 {
                    continue;
                }
                for c in c0..=c1 {
                    let cx0 = ox + c as f32 * cw;
                    let wx = overlap(x0, x1, cx0, cx0 + cw);
                    if wx <= 0.0 {
                        continue;
                    }
                    sum[r * cols + c] += l * (wx * wy) as f64;
                }
            }
        }
    }
    let area = (cw * ch) as f64;
    sum.into_iter().map(|s| (s / area) as f32).collect()
}

/// Pearson correlation over two equal-length populations, in `f64`. `NaN` when either
/// side has no variance — a black frame, a single-colour fixture — which every threshold
/// comparison treats as a fail, and which [`assess`] reports as `None` where the
/// question is meaningful (`correlation_lit`) and as a failing `NaN` where it is not.
pub fn pearson(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let n = a.len() as f64;
    if n < 2.0 {
        return f32::NAN;
    }
    let ma = a.iter().map(|v| *v as f64).sum::<f64>() / n;
    let mb = b.iter().map(|v| *v as f64).sum::<f64>() / n;
    let (mut cov, mut va, mut vb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b) {
        let dx = *x as f64 - ma;
        let dy = *y as f64 - mb;
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    if va <= 0.0 || vb <= 0.0 {
        return f32::NAN;
    }
    (cov / (va * vb).sqrt()) as f32
}

/// The pass/fail lines. **Parameters, not constants**: the defaults below are what the
/// synthetic self-test brackets (a perfect render passes with margin, a one-cell blur and
/// a scramble each fail their own law), and they are a starting point for T3's preset
/// gate, not its verdict. Where they should eventually live is beside the gate's goldens
/// (`native/verify/`), never in the param chain — a threshold is a test's opinion, not a
/// look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// Law 2: `correlation` (all cells) must be at least this.
    pub min_correlation: f32,
    /// Law 1, local: `bleed_max` must be at most this — the largest fraction of a lit
    /// neighbour's light found in a cell that was told to be dark.
    pub max_bleed: f32,
    /// Law 1, global: `stray_fraction` must be at most this — the share of the grid's
    /// energy in blank cells.
    pub max_stray: f32,
}

impl Default for Thresholds {
    fn default() -> Thresholds {
        Thresholds { min_correlation: 0.90, max_bleed: 0.25, max_stray: 0.10 }
    }
}

/// What [`assess`] measured, and how it was judged.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub cols: usize,
    pub rows: usize,
    /// Per-cell mean linear luma, row-major — the downsample. Kept so a failing report
    /// can be printed as a picture.
    pub measured: Vec<f32>,
    /// Per-cell expected linear luma from the fixture.
    pub expected: Vec<f32>,
    /// Pearson over every cell, blanks included. `NaN` if either side is constant.
    pub correlation: f32,
    /// Pearson over lit cells only — the gradient's shape. `None` when the fixture lights
    /// fewer than two cells or lights them all one colour (no variance to correlate).
    pub correlation_lit: Option<f32>,
    /// Max over blank cells with a lit 8-neighbour of `measured / mean(lit neighbours)`.
    pub bleed_max: f32,
    /// The `(col, row)` where `bleed_max` was found, if any blank cell had a lit neighbour.
    pub bleed_at: Option<(usize, usize)>,
    /// Energy in blank cells ÷ energy in all cells. `0` for a black frame.
    pub stray_fraction: f32,
    pub mean_lit: f32,
    pub mean_blank: f32,
    pub lit_cells: usize,
    pub thresholds: Thresholds,
}

impl Report {
    pub fn correlation_ok(&self) -> bool {
        self.correlation >= self.thresholds.min_correlation
    }
    pub fn bleed_ok(&self) -> bool {
        self.bleed_max <= self.thresholds.max_bleed
    }
    pub fn stray_ok(&self) -> bool {
        self.stray_fraction <= self.thresholds.max_stray
    }
    /// All three laws hold. `NaN` anywhere is a fail, by the comparisons above.
    pub fn pass(&self) -> bool {
        self.correlation_ok() && self.bleed_ok() && self.stray_ok()
    }

    /// One line, the numbers and their verdicts.
    pub fn summary(&self) -> String {
        let tick = |ok: bool| if ok { "ok" } else { "FAIL" };
        let lit = match self.correlation_lit {
            Some(c) => format!("{c:.3}"),
            None => "n/a".into(),
        };
        let at = match self.bleed_at {
            Some((c, r)) => format!(" at ({c},{r})"),
            None => String::new(),
        };
        format!(
            "legibility {}x{}: corr {:.4} (>= {:.2} {}) · lit-only {} · bleed {:.3}{} (<= {:.2} {}) · stray {:.4} (<= {:.2} {}) · lit {}/{} mean {:.4} blank mean {:.5} · {}",
            self.cols,
            self.rows,
            self.correlation,
            self.thresholds.min_correlation,
            tick(self.correlation_ok()),
            lit,
            self.bleed_max,
            at,
            self.thresholds.max_bleed,
            tick(self.bleed_ok()),
            self.stray_fraction,
            self.thresholds.max_stray,
            tick(self.stray_ok()),
            self.lit_cells,
            self.cols * self.rows,
            self.mean_lit,
            self.mean_blank,
            if self.pass() { "PASS" } else { "FAIL" },
        )
    }
}

/// **The entry point.** Score an image against a fixture at a known grid geometry.
///
/// `image` in linear light, row 0 at the top; `geom` says where the fixture's cell `(0, 0)`
/// is and how big a cell is (take the aspect from the fixture — [`GridGeom::at`] does).
/// Returns every number described in the module doc, judged against `thresholds`.
pub fn assess(image: &Image, geom: &GridGeom, fixture: &Fixture, thresholds: Thresholds) -> Report {
    let (cols, rows) = (fixture.cols, fixture.rows);
    let measured = downsample(image, geom, cols, rows);
    let expected = fixture.expected_luma();
    let lit: Vec<bool> = fixture.cells().iter().map(Cell::is_lit).collect();

    let correlation = pearson(&measured, &expected);
    let (lm, le): (Vec<f32>, Vec<f32>) = measured
        .iter()
        .zip(&expected)
        .zip(&lit)
        .filter(|(_, l)| **l)
        .map(|((m, e), _)| (*m, *e))
        .unzip();
    let correlation_lit = if lm.len() >= 2 {
        let c = pearson(&lm, &le);
        // NaN here means the expected side is constant (one colour): not a fail, no answer.
        if c.is_nan() && le.iter().all(|e| (*e - le[0]).abs() < 1e-7) { None } else { Some(c) }
    } else {
        None
    };

    let mut bleed_max = 0.0f32;
    let mut bleed_at = None;
    let (mut e_lit, mut e_blank, mut n_lit, mut n_blank) = (0.0f64, 0.0f64, 0usize, 0usize);
    for r in 0..rows {
        for c in 0..cols {
            let i = r * cols + c;
            if lit[i] {
                e_lit += measured[i] as f64;
                n_lit += 1;
                continue;
            }
            e_blank += measured[i] as f64;
            n_blank += 1;
            let (mut nsum, mut nn) = (0.0f64, 0usize);
            for dr in -1i64..=1 {
                for dc in -1i64..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let (nr, nc) = (r as i64 + dr, c as i64 + dc);
                    if nr < 0 || nc < 0 || nr >= rows as i64 || nc >= cols as i64 {
                        continue;
                    }
                    let j = nr as usize * cols + nc as usize;
                    if lit[j] {
                        nsum += measured[j] as f64;
                        nn += 1;
                    }
                }
            }
            if nn == 0 {
                continue;
            }
            let denom = nsum / nn as f64;
            let ratio = if denom > 0.0 { (measured[i] as f64 / denom) as f32 } else { 0.0 };
            if bleed_at.is_none() || ratio > bleed_max {
                bleed_max = ratio;
                bleed_at = Some((c, r));
            }
        }
    }
    let total = e_lit + e_blank;
    let stray_fraction = if total > 0.0 { (e_blank / total) as f32 } else { 0.0 };

    Report {
        cols,
        rows,
        measured,
        expected,
        correlation,
        correlation_lit,
        bleed_max,
        bleed_at,
        stray_fraction,
        mean_lit: if n_lit > 0 { (e_lit / n_lit as f64) as f32 } else { 0.0 },
        mean_blank: if n_blank > 0 { (e_blank / n_blank as f64) as f32 } else { 0.0 },
        lit_cells: n_lit,
        thresholds,
    }
}

/// **The entry point for a GPU readback whose grid fills the frame along at least one
/// axis.** Tightly packed sRGB RGBA8 (an `Rgba8UnormSrgb` target read back and
/// de-padded, as `tests/frame_boundary.rs` does), row 0 at the top; the grid is assumed
/// to be the largest centred fit at the fixture's aspect ([`GridGeom::fit`]). For
/// anything else — an HDR buffer (which is what a gain above 1 needs: 8 bits clip it),
/// a grid padded on both axes, a tilted camera — build an [`Image`] and a [`GridGeom`]
/// and call [`assess`]. Not wired anywhere yet: T3 decides where the gate lives.
pub fn assess_readback_rgba8(
    width: usize,
    height: usize,
    rgba8_srgb: &[u8],
    fixture: &Fixture,
    thresholds: Thresholds,
) -> Report {
    let image = Image::from_rgba8_srgb(width, height, rgba8_srgb);
    let geom = GridGeom::fit(width, height, fixture);
    assess(&image, &geom, fixture, thresholds)
}

/// Law 1 by isolation: for an image in which **only** cell `(col, row)` was lit, the
/// fraction of the image's total luma that landed outside that cell's footprint. This is
/// the spec's own phrasing of bleed — "the fraction of a lit cell's energy that lands
/// outside its own footprint" — and it needs a one-cell render, because a rendered pixel
/// does not say which cell it came from. `0` for a black image.
pub fn spill_fraction(image: &Image, geom: &GridGeom, col: usize, row: usize) -> f32 {
    let [x0, y0, x1, y1] = geom.cell_rect(col, row);
    let (mut inside, mut total) = (0.0f64, 0.0f64);
    for py in 0..image.height {
        let wy = overlap(py as f32, py as f32 + 1.0, y0, y1);
        for px in 0..image.width {
            let l = image.luma(px, py) as f64;
            total += l;
            let wx = overlap(px as f32, px as f32 + 1.0, x0, x1);
            inside += l * (wx * wy) as f64;
        }
    }
    if total > 0.0 { (1.0 - inside / total) as f32 } else { 0.0 }
}

// ---------------------------------------------------------------------------------------
// The synthetic renderer
// ---------------------------------------------------------------------------------------

/// A CPU painter and four degradations, so the metric can be tested against inputs whose
/// right answer is known — without an adapter.
///
/// The painter is the *ideal* render: each glyph is its [`Glyph`] rectangle at the
/// fixture's cell aspect, filled flat with the decoded fg colour, area-weighted at
/// fractional pixel edges, on black. Everything that follows is a controlled departure
/// from it, and each maps to one law: **blur** is bleed (law 1), **scramble** is a
/// broken value channel (law 2), **noise** is both a little, and **gain** is what must
/// change *neither* — §4 puts the phosphor several times above paper white.
pub mod synth {
    use super::{glyph_shape, overlap, srgb_to_linear, Fixture, GridGeom, Image, XorShift};

    /// Paint `fixture` at `cell_w` pixels per cell (height from the fixture's aspect) with
    /// `margin` black pixels around the grid. Returns the image and the geometry it was
    /// painted at, so the test never has to guess where the grid landed.
    pub fn paint(fixture: &Fixture, cell_w: f32, margin: f32) -> (Image, GridGeom) {
        let geom = GridGeom::at([margin, margin], cell_w, fixture.aspect);
        let width = (2.0 * margin + cell_w * fixture.cols as f32).ceil() as usize;
        let height = (2.0 * margin + geom.cell_h * fixture.rows as f32).ceil() as usize;
        let mut img = Image::black(width, height);
        for r in 0..fixture.rows {
            for c in 0..fixture.cols {
                let cell = fixture.cell(c, r);
                let Some(g) = glyph_shape(cell.symbol) else { continue };
                let [cx0, cy0, _, _] = geom.cell_rect(c, r);
                let (rx0, rx1) = (cx0 + g.x0 * geom.cell_w, cx0 + g.x1 * geom.cell_w);
                let (ry0, ry1) = (cy0 + g.y0 * geom.cell_h, cy0 + g.y1 * geom.cell_h);
                let lin = [
                    srgb_to_linear(cell.fg[0]) * g.intensity,
                    srgb_to_linear(cell.fg[1]) * g.intensity,
                    srgb_to_linear(cell.fg[2]) * g.intensity,
                ];
                let py0 = ry0.floor().max(0.0) as usize;
                let py1 = (ry1.ceil() as usize).min(height);
                let px0 = rx0.floor().max(0.0) as usize;
                let px1 = (rx1.ceil() as usize).min(width);
                for py in py0..py1 {
                    let wy = overlap(py as f32, py as f32 + 1.0, ry0, ry1);
                    for px in px0..px1 {
                        let w = wy * overlap(px as f32, px as f32 + 1.0, rx0, rx1);
                        if w <= 0.0 {
                            continue;
                        }
                        let p = &mut img.rgb[py * width + px];
                        for k in 0..3 {
                            p[k] += lin[k] * w;
                        }
                    }
                }
            }
        }
        (img, geom)
    }

    /// Gaussian blur with `sigma_cells` measured in **cells** — σ is `sigma_cells·cell_w`
    /// horizontally and `sigma_cells·cell_h` vertically, so "a blur of one cell" is
    /// isotropic on the grid and anisotropic in pixels, as a halation authored for a 2:1
    /// cell would be. Separable, kernel truncated at 3σ, edges clamped. Energy-preserving
    /// away from the image border, so what leaves a cell arrives in a neighbour.
    pub fn blur(image: &Image, geom: &GridGeom, sigma_cells: f32) -> Image {
        if sigma_cells <= 0.0 {
            return image.clone();
        }
        let pass = |src: &Image, sigma: f32, horizontal: bool| -> Image {
            let radius = (3.0 * sigma).ceil().max(1.0) as i64;
            let kernel: Vec<f32> = (-radius..=radius)
                .map(|i| (-(i as f32).powi(2) / (2.0 * sigma * sigma)).exp())
                .collect();
            let norm: f32 = kernel.iter().sum();
            let mut out = Image::black(src.width, src.height);
            for y in 0..src.height {
                for x in 0..src.width {
                    let mut acc = [0.0f32; 3];
                    for (k, wk) in kernel.iter().enumerate() {
                        let d = k as i64 - radius;
                        let (sx, sy) = if horizontal {
                            ((x as i64 + d).clamp(0, src.width as i64 - 1) as usize, y)
                        } else {
                            (x, (y as i64 + d).clamp(0, src.height as i64 - 1) as usize)
                        };
                        let p = src.rgb[sy * src.width + sx];
                        for c in 0..3 {
                            acc[c] += p[c] * wk;
                        }
                    }
                    out.rgb[y * src.width + x] = [acc[0] / norm, acc[1] / norm, acc[2] / norm];
                }
            }
            out
        };
        let h = pass(image, sigma_cells * geom.cell_w, true);
        pass(&h, sigma_cells * geom.cell_h, false)
    }

    /// Additive uniform noise in `[-amplitude, amplitude]` per channel, clamped at zero
    /// (light is not negative), seeded so every run sees the same grain.
    pub fn noise(image: &Image, amplitude: f32, seed: u64) -> Image {
        let mut rng = XorShift::new(seed);
        let mut out = image.clone();
        for p in &mut out.rgb {
            for c in p.iter_mut() {
                *c = (*c + amplitude * rng.signed_unit()).max(0.0);
            }
        }
        out
    }

    /// Multiply every channel by `k` — the emission gain of §4. Nothing the metric reports
    /// may move.
    pub fn gain(image: &Image, k: f32) -> Image {
        let mut out = image.clone();
        for p in &mut out.rgb {
            for c in p.iter_mut() {
                *c *= k;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------
// Unit tests — the pieces. The chain is tested in `tests/legibility.rs`.
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const ASYM: &str = include_str!("../tests/fixtures/asymmetric.txt");

    #[test]
    fn srgb_decode_matches_the_standard_at_its_anchors() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
        // sRGB 128 is ~0.2159 linear — the "mid-grey is a fifth" fact §4 rests on.
        assert!((srgb_to_linear(128) - 0.2159).abs() < 5e-4, "{}", srgb_to_linear(128));
        // Round trip.
        for v in [0u8, 1, 12, 64, 128, 200, 254, 255] {
            let back = (linear_to_srgb_f(srgb_to_linear(v)) * 255.0).round() as u8;
            assert_eq!(back, v);
        }
    }

    #[test]
    fn luma_is_rec709() {
        assert!((luma709([1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
        assert!((luma709([0.0, 1.0, 0.0]) - 0.7152).abs() < 1e-6);
    }

    #[test]
    fn glyph_coverage_is_the_fraction_of_the_cell_lit() {
        let cov = |s: char| glyph_shape(s).map(|g| g.coverage());
        assert_eq!(cov(' '), None);
        assert_eq!(cov('█'), Some(1.0));
        assert_eq!(cov('▀'), Some(0.5));
        assert_eq!(cov('▄'), Some(0.5));
        assert_eq!(cov('▌'), Some(0.5));
        assert_eq!(cov('▒'), Some(0.5));
        assert_eq!(cov('░'), Some(0.25));
        assert_eq!(cov('▓'), Some(0.75));
        assert_eq!(cov('▁'), Some(0.125));
        assert_eq!(cov('▇'), Some(0.875));
        assert_eq!(cov('▏'), Some(0.125));
        assert_eq!(cov('▉'), Some(0.875));
        assert_eq!(cov('▖'), Some(0.25));
        // The named approximation: a letterform is a full cell.
        assert_eq!(cov('A'), Some(1.0));
        // ▀ is the TOP half: y from 0 (top) to 0.5.
        let top = glyph_shape('▀').unwrap();
        assert_eq!((top.y0, top.y1), (0.0, 0.5));
        let bottom = glyph_shape('▄').unwrap();
        assert_eq!((bottom.y0, bottom.y1), (0.5, 1.0));
    }

    #[test]
    fn asymmetric_fixture_parses_as_drawn() {
        let f = Fixture::parse(ASYM).unwrap();
        assert_eq!((f.cols, f.rows, f.aspect), (5, 3, 2.0));
        assert_eq!(f.cell(0, 0), &Cell { symbol: '█', fg: [0xff, 0x30, 0x00] });
        assert_eq!(f.cell(1, 0), &Cell { symbol: '▀', fg: [0xff, 0x90, 0x00] });
        assert_eq!(f.cell(3, 1), &Cell { symbol: '▒', fg: [0x40, 0x90, 0xff] });
        assert_eq!(f.cell(3, 2), &Cell { symbol: '█', fg: [0xff, 0x30, 0x00] });
        assert_eq!(f.cell(4, 2).symbol, ' ');
        assert_eq!(f.lit_count(), 8);
        assert_eq!(f.census()[&' '], 7);
    }

    #[test]
    fn bottom_up_order_flips_the_rows() {
        let top_down = Fixture::parse(ASYM).unwrap();
        let bottom_up = Fixture::parse(&ASYM.replace("order top-down", "order bottom-up")).unwrap();
        assert_ne!(top_down, bottom_up, "the asymmetric fixture must change under a flip");
        for r in 0..3 {
            for c in 0..5 {
                assert_eq!(top_down.cell(c, r), bottom_up.cell(c, 2 - r), "({c},{r})");
            }
        }
        // And the same picture written bottom-up — lines reversed, in both blocks —
        // parses to the identical fixture.
        let mut lines: Vec<&str> = ASYM.lines().collect();
        let g = lines.iter().position(|l| l.trim() == "glyphs").unwrap();
        let k = lines.iter().position(|l| l.trim() == "colours").unwrap();
        lines[g + 1..g + 4].reverse();
        lines[k + 1..k + 4].reverse();
        let rewritten = lines.join("\n").replace("order top-down", "order bottom-up");
        assert_eq!(Fixture::parse(&rewritten).unwrap(), top_down);
    }

    #[test]
    fn crlf_parses_identically() {
        let crlf = ASYM.replace('\n', "\r\n");
        assert_eq!(Fixture::parse(&crlf).unwrap(), Fixture::parse(ASYM).unwrap());
    }

    #[test]
    fn parse_errors_name_the_line() {
        let e = |s: String| Fixture::parse(&s).unwrap_err();
        let msg = e(ASYM.replace("order top-down\n", ""));
        assert!(msg.msg.contains("missing `order`"), "{msg}");
        let msg = e(ASYM.replace("|█  ▒ |", "|█  ▒|"));
        assert!(msg.msg.contains("4 symbols") && msg.line > 0, "{msg}");
        let msg = e(ASYM.replace("|gobr |", "|gobz |"));
        assert!(msg.msg.contains("`z` is not declared"), "{msg}");
        let msg = e(ASYM.replace("aspect 2", "aspect -1"));
        assert!(msg.msg.contains("aspect"), "{msg}");
        let msg = e(ASYM.replace("grid v1", "grid v2"));
        assert!(msg.msg.contains("grid v1") && msg.line == 14, "{msg}");
        let msg = e(ASYM.replace("rows 3", "rows 4"));
        assert!(msg.msg.contains("3 fenced rows, `rows` says 4"), "{msg}");
    }

    #[test]
    fn scramble_keeps_every_cell_and_moves_them() {
        let f = Fixture::parse(ASYM).unwrap();
        let s = f.scrambled(7);
        assert_eq!(f.census(), s.census());
        assert_eq!(f.lit_count(), s.lit_count());
        assert_ne!(f, s);
        assert_eq!(s, f.scrambled(7), "seeded, so repeatable");
    }

    #[test]
    fn pearson_basics() {
        assert!((pearson(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]) - 1.0).abs() < 1e-6);
        assert!((pearson(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]) + 1.0).abs() < 1e-6);
        assert!(pearson(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]).is_nan());
        assert!(pearson(&[1.0], &[1.0]).is_nan());
        // Gain invariance, exactly the property §4 needs.
        let a = [0.1f32, 0.7, 0.2, 0.9];
        let b: Vec<f32> = a.iter().map(|v| v * 6.0).collect();
        assert!((pearson(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn downsample_row_zero_is_the_top_of_the_image() {
        // 2 cols × 2 rows of 2×4 px cells (aspect 2); only the top-left cell lit.
        let mut img = Image::black(4, 8);
        for y in 0..4 {
            for x in 0..2 {
                img.rgb[y * 4 + x] = [1.0, 1.0, 1.0];
            }
        }
        let geom = GridGeom::at([0.0, 0.0], 2.0, 2.0);
        let m = downsample(&img, &geom, 2, 2);
        assert_eq!(m, vec![1.0, 0.0, 0.0, 0.0], "cell (0,0) is the TOP-left");
    }

    #[test]
    fn downsample_splits_a_straddling_pixel_by_area() {
        // One row, two cells 1.5 px wide over a 3 px image; the middle pixel is half in each.
        let mut img = Image::black(3, 1);
        img.rgb[1] = [1.0, 1.0, 1.0];
        let geom = GridGeom::at([0.0, 0.0], 1.5, 1.0 / 1.5);
        let m = downsample(&img, &geom, 2, 1);
        // Each cell gets 0.5 px of luma over an area of 1.5 px.
        assert!((m[0] - 0.5 / 1.5).abs() < 1e-6 && (m[1] - 0.5 / 1.5).abs() < 1e-6, "{m:?}");
    }

    #[test]
    fn fit_centres_the_largest_grid_at_the_fixture_aspect() {
        let f = Fixture::parse(ASYM).unwrap(); // 5 × 3 at 2:1 → grid is 5w × 6w
        let g = GridGeom::fit(100, 60, &f); // height-bound: cell_w = 10
        assert_eq!((g.cell_w, g.cell_h), (10.0, 20.0));
        assert_eq!(g.origin, [25.0, 0.0]);
        let g = GridGeom::fit(50, 600, &f); // width-bound
        assert_eq!((g.cell_w, g.cell_h), (10.0, 20.0));
        assert_eq!(g.origin, [0.0, 270.0]);
    }

    #[test]
    fn image_decoders_agree() {
        let bytes = [128u8, 255, 0, 255];
        let a = Image::from_rgba8_srgb(1, 1, &bytes);
        let f = [srgb_to_linear(128), 1.0, 0.0, 1.0];
        let b = Image::from_rgba_f32(1, 1, &f);
        let h: Vec<u16> = f.iter().map(|v| half::f16::from_f32(*v).to_bits()).collect();
        let c = Image::from_rgba16f(1, 1, &h);
        for k in 0..3 {
            assert!((a.rgb[0][k] - b.rgb[0][k]).abs() < 1e-6);
            assert!((a.rgb[0][k] - c.rgb[0][k]).abs() < 2e-3, "half precision");
        }
        let img = image::RgbaImage::from_raw(1, 1, bytes.to_vec()).unwrap();
        assert_eq!(Image::from(&img), a);
        // Encode clips: 4× white comes back as 255.
        let hot = Image::from_rgba_f32(1, 1, &[4.0, 4.0, 4.0, 1.0]);
        assert_eq!(&hot.to_rgba8_srgb()[..3], &[255, 255, 255]);
    }

    #[test]
    fn lit_only_correlation_is_none_when_every_lit_cell_expects_the_same() {
        // One colour AND one coverage: nothing to correlate, and that is `None`, not a fail.
        let mut cells = vec![Cell::BLANK; 6];
        cells[0] = Cell { symbol: '█', fg: [200, 200, 200] };
        cells[4] = Cell { symbol: '█', fg: [200, 200, 200] };
        let f = Fixture::from_cells(3, 2, 2.0, cells);
        let (img, geom) = synth::paint(&f, 4.0, 2.0);
        let r = assess(&img, &geom, &f, Thresholds::default());
        assert_eq!(r.correlation_lit, None);
        assert!(r.pass(), "{}", r.summary());
        // Swap one for a half block and the question exists again.
        let mut cells = f.cells().to_vec();
        cells[4].symbol = '▄';
        let f2 = Fixture::from_cells(3, 2, 2.0, cells);
        let (img, geom) = synth::paint(&f2, 4.0, 2.0);
        let r = assess(&img, &geom, &f2, Thresholds::default());
        assert!(r.correlation_lit.unwrap() > 0.9999, "{}", r.summary());
    }

    #[test]
    fn thresholds_are_parameters() {
        let f = Fixture::parse(ASYM).unwrap();
        let (img, geom) = synth::paint(&f, 8.0, 4.0);
        let lenient = assess(&img, &geom, &f, Thresholds::default());
        assert!(lenient.pass(), "{}", lenient.summary());
        let impossible = assess(&img, &geom, &f, Thresholds { min_correlation: 1.01, ..Thresholds::default() });
        assert!(!impossible.pass(), "a threshold above 1 must fail a perfect render");
        assert_eq!(lenient.correlation, impossible.correlation, "the number is the same; only the verdict moved");
    }
}
