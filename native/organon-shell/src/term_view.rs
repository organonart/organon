//! The glyph grid, drawn with egui (Shell #10 Tier 1).
//!
//! T1 renders through egui's text painter — per-line runs of same-styled glyphs,
//! background rects underneath, a block cursor on top. Honest scope note: the
//! dedicated glyph-atlas instanced pipeline (the perf ceiling) is a later tier of
//! #10; this pass is correctness and feel, and egui on an M-series GPU holds a
//! full-window grid comfortably. Everything color is resolved here: the default
//! look is the PRD's "restrained phosphor-tinged" dark, and cells carry
//! `vte::ansi::Color` values resolved against the ANSI/256 palette plus any
//! OSC 4/10/11 overrides the application installed (`content.colors`).

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use crate::term::{self, TermSession};

/// The default screen: near-black with a whisper of green, phosphor foreground.
pub const DEFAULT_BG: egui::Color32 = egui::Color32::from_rgb(0x0a, 0x0d, 0x0a);
pub const DEFAULT_FG: egui::Color32 = egui::Color32::from_rgb(0xc8, 0xe6, 0xc8);

/// `ORGANON_SHELL_SCRIM`'s default and its structural floor, as an **alpha byte** —
/// the scrim is an `egui::Color32` alpha, so the scale is 0–255, not 0–1.
///
/// ⚠️ **These are `pub` so `--help` can quote them rather than restate them.** The first
/// draft of `organon-shell --help` documented `ORGANON_SHELL_SCRIM=<0..1>` from memory. That
/// is not a cosmetic slip: `0.5` fails the `u8` parse, `.ok()` swallows the error, and the
/// value silently falls back to the default — so a user following the docs exactly would see
/// no effect and no complaint. The review that caught it noted the irony, this PR existing to
/// fix docs a stranger would follow. Formatting the help from the constants makes that class
/// of drift impossible rather than merely fixed once.
pub const SCRIM_DEFAULT: u8 = 185;
/// The floor is the inviolable half of PRD §4.6: no setting may trade the glyphs away.
pub const SCRIM_FLOOR: u8 = 96;

/// The scrim's alpha for a given `ORGANON_SHELL_SCRIM` value — parse, default, **floor**.
/// `None` is "unset".
///
/// Extracted from [`draw`] by the Console Spike's Tier 1 (brief R1 (b)): the floor is the one
/// inviolable rule in this file and it lived in an expression nothing could reach. As a
/// function it is a test — over every byte, and over the inputs that do *not* parse.
///
/// ⚠️ **A value that fails the `u8` parse falls back to the DEFAULT, not to the floor.** `300`,
/// `-1`, `0.5` and `abc` are all "unset" as far as this is concerned, which is the same
/// swallowing that made `--help`'s original `<0..1>` a silent no-op. It is the tolerant
/// behaviour and it is deliberate — but it means the floor is what protects the glyphs, and the
/// default is merely where a typo lands.
pub fn scrim_alpha(env: Option<&str>) -> u8 {
    env.and_then(|v| v.parse::<u8>().ok()).unwrap_or(SCRIM_DEFAULT).max(SCRIM_FLOOR)
}

/// The 16 ANSI colors, phosphor-leaning but conventional enough that TUI color
/// schemes read as intended.
const ANSI16: [egui::Color32; 16] = [
    egui::Color32::from_rgb(0x10, 0x14, 0x10), // black
    egui::Color32::from_rgb(0xcc, 0x52, 0x4b), // red
    egui::Color32::from_rgb(0x5c, 0xb8, 0x5c), // green
    egui::Color32::from_rgb(0xc2, 0xb0, 0x4c), // yellow
    egui::Color32::from_rgb(0x56, 0x92, 0xd8), // blue
    egui::Color32::from_rgb(0xb0, 0x6c, 0xc0), // magenta
    egui::Color32::from_rgb(0x4c, 0xb8, 0xb0), // cyan
    egui::Color32::from_rgb(0xc8, 0xd2, 0xc8), // white
    egui::Color32::from_rgb(0x50, 0x5a, 0x50), // bright black
    egui::Color32::from_rgb(0xe8, 0x6a, 0x62), // bright red
    egui::Color32::from_rgb(0x74, 0xd8, 0x74), // bright green
    egui::Color32::from_rgb(0xdc, 0xcc, 0x66), // bright yellow
    egui::Color32::from_rgb(0x74, 0xac, 0xec), // bright blue
    egui::Color32::from_rgb(0xcc, 0x88, 0xdc), // bright magenta
    egui::Color32::from_rgb(0x68, 0xd4, 0xcc), // bright cyan
    egui::Color32::from_rgb(0xee, 0xf4, 0xee), // bright white
];

/// Resolve a cell color against overrides → named table → 256-cube → truecolor.
fn resolve(
    color: AnsiColor,
    overrides: &alacritty_terminal::term::color::Colors,
    default_fg: bool,
) -> egui::Color32 {
    match color {
        AnsiColor::Spec(rgb) => egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Named(named) => {
            if let Some(rgb) = overrides[named as usize] {
                return egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b);
            }
            match named {
                NamedColor::Foreground => DEFAULT_FG,
                NamedColor::Background => DEFAULT_BG,
                n => {
                    let i = n as usize;
                    if i < 16 {
                        ANSI16[i]
                    } else if default_fg {
                        DEFAULT_FG
                    } else {
                        DEFAULT_BG
                    }
                }
            }
        }
        AnsiColor::Indexed(i) => {
            if let Some(rgb) = overrides[i as usize] {
                return egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b);
            }
            indexed_256(i)
        }
    }
}

/// The standard xterm 256-color table: 16 ANSI + 6×6×6 cube + 24 grays.
fn indexed_256(i: u8) -> egui::Color32 {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let i = i as u16 - 16;
            let comp = |v: u16| -> u8 {
                if v == 0 {
                    0
                } else {
                    (55 + v * 40) as u8
                }
            };
            egui::Color32::from_rgb(
                comp(i / 36),
                comp((i / 6) % 6),
                comp(i % 6),
            )
        }
        232..=255 => {
            let g = 8 + (i as u16 - 232) * 10;
            egui::Color32::from_rgb(g as u8, g as u8, g as u8)
        }
    }
}

/// One frame of the terminal: pump the session, size the grid to the rect, feed
/// input, paint. The caller gives us the whole window (PRD v3 §7.5: no chrome).
///
/// `backdrop` is tree E Tier 1 (the engine behind the glyphs): when `Some`, the
/// caller has already rendered the world into that texture this frame, and it is
/// painted under everything with the **legibility scrim** over it — PRD §4.6's
/// inviolable rule, enforced here structurally rather than by taste: whatever the
/// engine shows, the glyph layer keeps its contrast floor.
pub fn draw(ui: &mut egui::Ui, session: &mut TermSession, backdrop: Option<egui::TextureId>) {
    let font_id = egui::FontId::monospace(14.0);
    let (cell_w, cell_h) =
        ui.fonts_mut(|f| (f.glyph_width(&font_id, 'M'), f.row_height(&font_id)));

    let rect = ui.available_rect_before_wrap();
    let cols = (rect.width() / cell_w).floor().max(2.0) as u16;
    let rows = (rect.height() / cell_h).floor().max(2.0) as u16;
    // Grid metrics, reported on change under ORGANON_SHELL_PTY_DEBUG. A blank grid
    // has two very different causes — no bytes, or a grid so mis-measured that the
    // bytes land off-screen (a fallback font making `cell_w` absurd would do it) —
    // and this is the half of that question the PTY trace cannot answer.
    //
    // ⚠️ Scoped PER SESSION, via the size this session already carries — never a
    // `static`. A process-wide dedup key would swallow the *second* tab's line
    // whenever two tabs agree on `cols`x`rows`, which is the common case (same
    // window, same font) and is precisely the comparison this instrument exists to
    // make. A diagnostic that silently drops a reading is worse than none.
    // Read before `resize` below overwrites it.
    if term::pty_debug() && (cols, rows) != session.size() {
        eprintln!(
            "[grid] {cols}x{rows}  cell={cell_w:.2}x{cell_h:.2}  rect={:.0}x{:.0}",
            rect.width(),
            rect.height()
        );
    }
    session.resize(cols, rows);
    session.pump();

    // ── Input ──────────────────────────────────────────────────────────────
    // The terminal owns the keyboard, full stop (T1: no other widget exists).
    let app_cursor = session.term.mode().contains(TermMode::APP_CURSOR);
    let events = ui.input(|i| i.events.clone());
    for event in &events {
        match event {
            egui::Event::Text(text) => {
                // Printable input. Ctrl-modified letters arrive as `Key` events,
                // not Text, so this is exactly the non-control stream.
                session.input(text.as_bytes());
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                // ⌘ belongs to the terminal's chrome (tabs — the macOS terminal
                // convention); it never reaches the PTY. tabs::command_key_action
                // is the other half of this contract.
                if modifiers.command {
                    continue;
                }
                if let Some(bytes) = term::encode_key(*key, *modifiers, app_cursor) {
                    session.input(&bytes);
                }
            }
            egui::Event::Paste(text) => {
                // Bracketed paste when the app asked for it, raw bytes otherwise.
                if session.term.mode().contains(TermMode::BRACKETED_PASTE) {
                    session.input(b"\x1b[200~");
                    session.input(text.as_bytes());
                    session.input(b"\x1b[201~");
                } else {
                    session.input(text.as_bytes());
                }
            }
            _ => {}
        }
    }
    let scroll = ui.input(|i| i.raw_scroll_delta.y);
    if scroll.abs() >= 1.0 {
        session.scroll_display((scroll / cell_h * 1.5) as i32);
    }

    // ── Paint ──────────────────────────────────────────────────────────────
    let painter = ui.painter_at(rect);
    match backdrop {
        Some(texture) => {
            painter.image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            // The legibility scrim over the render: the engine glows through, the
            // text never fights it. `ORGANON_SHELL_SCRIM` tunes the reveal — but the
            // floor is structural, so no setting can trade the glyphs away
            // (PRD §4.6, the inviolable half). See [`scrim_alpha`], which owns the rule.
            let scrim = scrim_alpha(std::env::var("ORGANON_SHELL_SCRIM").ok().as_deref());
            painter.rect_filled(
                rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0x0a, 0x0d, 0x0a, scrim),
            );
        }
        None => {
            painter.rect_filled(rect, 0.0, DEFAULT_BG);
        }
    }

    let content = session.term.renderable_content();
    let display_offset = content.display_offset as i32;
    let colors = content.colors;
    let cursor = content.cursor;

    // Runs of same-styled glyphs per line — one text galley per run, one bg rect
    // per run, instead of a call per cell.
    let mut run = String::new();
    let mut run_start: Option<(f32, f32)> = None;
    let mut run_fg = DEFAULT_FG;
    let mut run_bg = DEFAULT_BG;
    let mut last: Option<(i32, usize)> = None;

    let mut flush =
        |run: &mut String, start: &mut Option<(f32, f32)>, fg: egui::Color32, bg: egui::Color32| {
            if let Some((x, y)) = start.take() {
                if !run.is_empty() {
                    let w = run.chars().count() as f32 * cell_w;
                    if bg != DEFAULT_BG {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(x, y),
                                egui::vec2(w, cell_h),
                            ),
                            0.0,
                            bg,
                        );
                    }
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_TOP,
                        run.as_str(),
                        font_id.clone(),
                        fg,
                    );
                    run.clear();
                }
            }
        };

    for indexed in content.display_iter {
        let point = indexed.point;
        // Grid line → viewport row. Lines are grid-relative (negative = history);
        // the display offset shifts the visible window up into scrollback.
        let vrow = point.line.0 + display_offset;
        if vrow < 0 || vrow >= rows as i32 {
            continue;
        }
        let col = point.column.0;
        let cell = &indexed.cell;
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }

        let (mut fg, mut bg) = (
            resolve(cell.fg, colors, true),
            resolve(cell.bg, colors, false),
        );
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.6);
        }

        let contiguous = last == Some((vrow, col.wrapping_sub(1)));
        if !contiguous || fg != run_fg || bg != run_bg {
            flush(&mut run, &mut run_start, run_fg, run_bg);
            run_start = Some((
                rect.left() + col as f32 * cell_w,
                rect.top() + vrow as f32 * cell_h,
            ));
            run_fg = fg;
            run_bg = bg;
        }
        run.push(if cell.c == '\0' { ' ' } else { cell.c });
        last = Some((vrow, col));
    }
    flush(&mut run, &mut run_start, run_fg, run_bg);

    // ── Cursor ─────────────────────────────────────────────────────────────
    // A filled block when at the live edge; hidden while scrolled into history
    // (the terminal convention). Blink is a later polish tier — presence first.
    let cur_row = cursor.point.line.0 + display_offset;
    if display_offset == 0 && cur_row >= 0 && cur_row < rows as i32 {
        let x = rect.left() + cursor.point.column.0 as f32 * cell_w;
        let y = rect.top() + cur_row as f32 * cell_h;
        let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
        painter.rect_filled(r, 0.0, DEFAULT_FG);
        // Repaint the glyph under the cursor in inverse.
        let ch = session.term.grid()[cursor.point].c;
        if ch != ' ' && ch != '\0' {
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                ch.to_string(),
                font_id.clone(),
                DEFAULT_BG,
            );
        }
    }

    if session.exited {
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, cell_h),
            egui::Align2::CENTER_BOTTOM,
            "[process exited — close the window or open a new tab]",
            font_id,
            egui::Color32::from_rgb(0x80, 0x8a, 0x80),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The floor holds against anything.** PRD §4.6's inviolable half, as a test rather than
    /// a comment: no `ORGANON_SHELL_SCRIM` value — in range, out of range, negative, empty,
    /// unset or nonsense — can put the scrim below [`SCRIM_FLOOR`] and trade the glyphs away.
    #[test]
    fn no_scrim_setting_can_cross_the_floor() {
        // The hostile set: the two ends of the byte scale, the forms that fail the parse (an
        // out-of-range number, a negative, the `<0..1>` spelling the first `--help` invented),
        // and unset.
        assert_eq!(scrim_alpha(Some("0")), SCRIM_FLOOR, "0 must be lifted to the floor");
        assert_eq!(scrim_alpha(Some("95")), SCRIM_FLOOR);
        assert_eq!(scrim_alpha(Some("255")), 255, "the top of the scale is honoured");
        assert_eq!(scrim_alpha(None), SCRIM_DEFAULT, "unset is the default");
        for junk in ["", " ", "abc", "0.5", "-1", "300", "96 ", "0x60"] {
            assert_eq!(scrim_alpha(Some(junk)), SCRIM_DEFAULT, "{junk:?} must fall back");
        }
        // And exhaustively over the whole byte scale — the property, not a sample of it.
        for v in 0u16..=255 {
            let a = scrim_alpha(Some(&v.to_string()));
            assert!(a >= SCRIM_FLOOR, "scrim {v} produced {a}, below the floor");
            assert_eq!(a, (v as u8).max(SCRIM_FLOOR));
        }
    }

    /// The 256-color cube math, pinned at its corners.
    #[test]
    fn xterm_256_table_corners() {
        assert_eq!(indexed_256(16), egui::Color32::from_rgb(0, 0, 0));
        assert_eq!(indexed_256(231), egui::Color32::from_rgb(255, 255, 255));
        assert_eq!(indexed_256(196), egui::Color32::from_rgb(255, 0, 0)); // pure red
        assert_eq!(indexed_256(232), egui::Color32::from_rgb(8, 8, 8)); // darkest gray
        assert_eq!(indexed_256(255), egui::Color32::from_rgb(238, 238, 238)); // lightest
    }
}
