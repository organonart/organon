//! The glyph-ring producer's library half (organon#217 T1): build a `ttfx` effect by
//! name, tick it headless under a virtual clock, and walk its cell grid into
//! [`GlyphCell`]s in the ring's orientation. The binary in `main.rs` is the loop around
//! this — pacing, the dwell, the next effect — and nothing here touches a clock or the
//! ring, which is what makes the walk testable against a real engine in milliseconds.
//!
//! ⚠️ **`Terminal.terminal_state` is the wrong field.** It is public and it is the
//! *formatted* rows — colour already folded into ANSI bytes. `render_cells` and
//! `visible_characters` are private. So [`walk_grid`] rebuilds the painter's walk from
//! public fields, matching `Terminal::update_render_cells` (`src/engine/terminal.rs`)
//! step for step: every `arena` entry with `is_visible`, positioned at
//! `motion.current_coord + canvas_{row,column}_offset`, clipped to the visible window,
//! and the max `(layer, character_id)` winning each cell. What it reads per winner —
//! `animation.current_character_visual` — is the same `CharacterVisual` the terminal
//! formats, minus the formatting.
//!
//! ⚠️ **ttfx rows grow UP** (`Coord`: 1-based, origin bottom-left). The ring is top-down.
//! The flip is `ring_row = height - row`, applied once, here, and pinned by a test on an
//! asymmetric fixture — a symmetric logo cannot tell a wrong flip from a right one.
//!
//! ⚠️ **Never call ttfx's signal plumbing** (`install_sigint_handler` and friends in its
//! `lib.rs`): it is Unix-only `signal(2)` and a library caller has no use for it. Nothing
//! here does, which is why this crate builds on Windows.
//!
//! **Persistence is a pass over the walk, not part of it** (T11, [`persist`]): the
//! walk stays the engine's truth, and [`Persistence::apply`] rewrites a copy of it into
//! what the ring carries. `walk_grid` never knows a phosphor exists.

pub mod persist;
pub use persist::{Persistence, PERSIST_FLOOR};

use clap::{CommandFactory, Parser};
use organon_core::glyph_ring::{
    pack_rgb, GlyphCell, SGR_ACTIVE_PATH, SGR_BLINK, SGR_BOLD, SGR_DIM, SGR_HAS_BG, SGR_HAS_FG,
    SGR_HIDDEN, SGR_ITALIC, SGR_REVERSE, SGR_STRIKE, SGR_UNDERLINE,
};
use ttfx::cli::Cli;
use ttfx::effects::EffectCommand;
use ttfx::engine::ctx::{Clock, EngineCtx};
use ttfx::engine::effect::Effect;
use ttfx::engine::terminal::{Terminal, TerminalConfig};
use ttfx::utils::rng::Rng;

/// Every effect ttfx registers, by subcommand name, in registry order. Read off the
/// clap command tree exactly as ttfx's `--random-effect` does, so a new upstream effect
/// appears here with no edit.
pub fn effect_names() -> Vec<String> {
    Cli::command().get_subcommands().map(|c| c.get_name().to_string()).collect()
}

/// Build an effect command from its name with pure default configuration — the way
/// ttfx's `main.rs` builds `--random-effect`. Unknown names are refused with the list.
pub fn effect_by_name(name: &str) -> Result<EffectCommand, String> {
    match Cli::try_parse_from(["ttfx", name]) {
        Ok(Cli { effect: Some(effect), .. }) => Ok(effect),
        _ => Err(format!(
            "unknown effect '{name}'; ttfx knows: {}",
            effect_names().join(", ")
        )),
    }
}

/// The headless terminal configuration: the canvas is the input's own size unless
/// `cols`/`rows` are given, and the real terminal (if any) is ignored — a producer
/// under a service manager has none, and ttfx would otherwise clip the canvas to
/// `(80, 24)` and centre it, silently.
pub fn terminal_config(frame_rate: i64, cols: Option<i64>, rows: Option<i64>) -> TerminalConfig {
    // `Cli::parse_from` with no arguments yields every terminal default in one place —
    // the same defaults the real CLI runs with — rather than a second hand-written copy
    // that could drift from ttfx's.
    let base = Cli::parse_from(["ttfx"]).terminal_config();
    TerminalConfig {
        frame_rate,
        canvas_width: cols.unwrap_or(-1),
        canvas_height: rows.unwrap_or(-1),
        ignore_terminal_dimensions: true,
        ..base
    }
}

/// A running effect: the engine context and the effect over it. `tick` counts
/// frames delivered so far; `done` is set once `next_frame` has returned `None`.
pub struct Producer {
    pub name: String,
    pub ctx: EngineCtx,
    effect: Box<dyn Effect>,
    pub tick: u32,
    pub done: bool,
}

impl Producer {
    /// Build `name` over `input`, seeded, at `frame_rate` virtual frames per second,
    /// and call `Effect::build`. The virtual clock is what makes the cadence ours: it
    /// steps a fixed `dt` per `next_frame` and never sleeps (§6).
    pub fn start(
        input: &str,
        name: &str,
        seed: u64,
        frame_rate: i64,
        cols: Option<i64>,
        rows: Option<i64>,
    ) -> Result<Producer, String> {
        let command = effect_by_name(name)?;
        let config = terminal_config(frame_rate, cols, rows);
        let clock = Clock::virtual_with_frame_rate(frame_rate);
        let mut ctx = EngineCtx::new(input, config, Rng::seeded(seed), clock)
            .map_err(|e| format!("engine: {e}"))?;
        let mut effect = command.build_effect();
        effect.build(&mut ctx).map_err(|e| format!("{name}: build: {e}"))?;
        Ok(Producer { name: name.to_string(), ctx, effect, tick: 0, done: false })
    }

    /// Advance one frame. `true` if the grid moved on; `false` once the effect has
    /// finished (the grid is then the settled text and stays readable via `walk`).
    /// The ANSI string `next_frame` returns is dropped unread — the cells are read
    /// from the engine instead.
    pub fn step(&mut self) -> bool {
        if self.done {
            return false;
        }
        match self.effect.next_frame(&mut self.ctx) {
            Some(frame) => {
                // ttfx recycles this String through `Terminal::recycle_output_string`,
                // which is `pub(crate)` — so from outside the crate each tick formats
                // and frees one frame's worth of ANSI we never read. Measured against
                // the walk it is noise; an upstream `pub` on that method is the fix.
                drop(frame);
                self.tick = self.tick.wrapping_add(1);
                true
            }
            None => {
                self.done = true;
                false
            }
        }
    }

    /// Walk the current grid into `out` (top-down, row-major). Returns `(cols, rows)`.
    pub fn walk(&self, out: &mut Vec<GlyphCell>) -> (u32, u32) {
        walk_grid(&self.ctx.terminal, out)
    }
}

/// The ring's sub-cell offset for a character (§7): `current_pos - current_coord`, the
/// remainder ttfx's banker's rounding dropped on the way to `current_coord`, in cells,
/// each axis in `-0.5..=0.5`. ttfx carries the pre-rounded point as
/// `Motion.current_pos` (organonart/ttfx PR — see `doc/pbr_text_engine.md` §7).
///
/// ⚠️ **No flip, unlike the row index.** ttfx's row grows UP, and the ring's `sub_y` is
/// "+y up from the cell's centre" (`GlyphCell`), so the remainder is carried as it is;
/// only the *cell index* is flipped top-down in [`walk_grid`]. A character placed by
/// `set_coordinate` has no remainder and encodes as exactly `(0.0, 0.0)`, so a cut is a
/// cut. `f64 → f32` is the only loss, and it is far below a pixel.
pub fn sub_cell_offset(motion: &ttfx::engine::motion::Motion) -> (f32, f32) {
    let (x, y) = motion.sub_cell();
    (x as f32, y as f32)
}

/// The painter's walk (see the module doc): fill `out` with `cols × rows` cells in the
/// ring's top-down orientation and return `(cols, rows)`.
pub fn walk_grid(term: &Terminal, out: &mut Vec<GlyphCell>) -> (u32, u32) {
    let width = term.visible_right.max(0) as usize;
    let height = term.visible_top.max(0) as usize;
    let n = width * height;
    // Winner per cell: the arena index of the max `(layer, character_id)` — the same
    // selection `update_render_cells` makes, and the reason two characters sharing a
    // cell render the one an effect put "on top".
    let mut winner: Vec<Option<usize>> = vec![None; n];
    for (i, ch) in term.arena.iter().enumerate() {
        if !ch.is_visible {
            continue;
        }
        let row = ch.motion.current_coord.row + term.canvas_row_offset;
        let column = ch.motion.current_coord.column + term.canvas_column_offset;
        if row < term.visible_bottom
            || row > term.visible_top
            || column < term.visible_left
            || column > term.visible_right
        {
            continue;
        }
        // ttfx: row 1 is the bottom. Ring: row 0 is the top.
        let ring_row = height - row as usize;
        let idx = ring_row * width + (column as usize - 1);
        match winner[idx] {
            None => winner[idx] = Some(i),
            Some(w) => {
                let painted = &term.arena[w];
                if (ch.layer, ch.character_id) > (painted.layer, painted.character_id) {
                    winner[idx] = Some(i);
                }
            }
        }
    }
    out.clear();
    out.reserve(n);
    for w in winner {
        out.push(match w {
            None => GlyphCell::default(),
            Some(i) => {
                let ch = &term.arena[i];
                let v = &ch.animation.current_character_visual;
                let mut sgr = 0;
                for (on, bit) in [
                    (v.bold, SGR_BOLD),
                    (v.dim, SGR_DIM),
                    (v.italic, SGR_ITALIC),
                    (v.underline, SGR_UNDERLINE),
                    (v.blink, SGR_BLINK),
                    (v.reverse, SGR_REVERSE),
                    (v.hidden, SGR_HIDDEN),
                    (v.strike, SGR_STRIKE),
                    (ch.motion.active_path.is_some(), SGR_ACTIVE_PATH),
                ] {
                    if on {
                        sgr |= bit;
                    }
                }
                let (mut fg, mut bg) = (0, 0);
                if let Some(colors) = &v.colors {
                    if let Some(c) = &colors.fg_color {
                        let (r, g, b) = c.rgb_ints();
                        fg = pack_rgb(r, g, b);
                        sgr |= SGR_HAS_FG;
                    }
                    if let Some(c) = &colors.bg_color {
                        let (r, g, b) = c.rgb_ints();
                        bg = pack_rgb(r, g, b);
                        sgr |= SGR_HAS_BG;
                    }
                }
                let (sub_x, sub_y) = sub_cell_offset(&ch.motion);
                GlyphCell {
                    symbol: v.symbol.chars().next().map(|c| c as u32).unwrap_or(0),
                    fg,
                    bg,
                    sgr,
                    layer: ch.layer as i32,
                    character_id: ch.character_id,
                    sub_x,
                    sub_y,
                }
            }
        });
    }
    (width as u32, height as u32)
}

/// Pick the next effect for a `--random` run: a uniform choice over `names` from the
/// producer's own seeded `Rng`, never repeating the previous pick when there is a
/// choice — a screensaver that plays the same effect twice running reads as stuck.
pub fn pick_next(rng: &mut Rng, names: &[String], previous: Option<&str>) -> String {
    if names.len() <= 1 {
        return names.first().cloned().unwrap_or_default();
    }
    loop {
        let n = names[rng.choice_index(names.len())].clone();
        if previous != Some(n.as_str()) {
            return n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use organon_core::glyph_ring::SGR_HAS_FG;

    /// The registry is read off ttfx's clap tree, so every name it yields must build.
    /// Thirty-seven at rev 7203e35 (§8 ran them all); the assertion is a floor, not a
    /// count, so an upstream addition does not fail it.
    #[test]
    fn every_registered_effect_builds_by_name() {
        let names = effect_names();
        assert!(names.len() >= 30, "{}", names.len());
        assert!(names.iter().any(|n| n == "beams"));
        for n in &names {
            effect_by_name(n).unwrap_or_else(|e| panic!("{n}: {e}"));
        }
        let e = effect_by_name("no-such-effect").unwrap_err();
        assert!(e.contains("unknown effect") && e.contains("beams"), "{e}");
    }

    /// The flip, pinned on an input that is NOT symmetric. ttfx lays "AB" on row 2 (the
    /// top, since rows count from the bottom) and "C" on row 1; the ring must hand
    /// "AB" out FIRST.
    #[test]
    fn the_walk_is_top_down_on_an_asymmetric_fixture() {
        let config = terminal_config(60, None, None);
        let mut term = Terminal::new("AB\nC", config).unwrap();
        let ids: Vec<_> = term.character_by_input_coord.values().copied().collect();
        for id in ids {
            term.set_character_visibility(id, true);
        }
        let mut cells = Vec::new();
        let (cols, rows) = walk_grid(&term, &mut cells);
        assert_eq!((cols, rows), (2, 2));
        let syms: Vec<char> = cells.iter().map(|c| char::from_u32(c.symbol).unwrap()).collect();
        // Fill characters are spaces; the ring stores them as 0x20, which `tile_for`
        // treats as empty.
        assert_eq!(syms, vec!['A', 'B', 'C', ' '], "row 0 must be the TOP line");
        assert_eq!(cells[0].character_id, term.arena[term.character_by_input_coord[&ttfx::utils::geometry::Coord::new(1, 2)].0 as usize].character_id);
    }

    /// The headless config must not consult the (absent) terminal: the canvas is the
    /// input's size and sits at offset 0, whatever `COLUMNS`/`LINES` or a tty would say.
    #[test]
    fn the_headless_canvas_is_the_input_size_at_the_origin() {
        let term = Terminal::new("abcdef\ng\nh", terminal_config(60, None, None)).unwrap();
        assert_eq!((term.visible_right, term.visible_top), (6, 3));
        assert_eq!((term.canvas_column_offset, term.canvas_row_offset), (0, 0));
        let sized = Terminal::new("ab", terminal_config(60, Some(10), Some(4))).unwrap();
        assert_eq!((sized.visible_right, sized.visible_top), (10, 4));
    }

    /// Drive a real effect to completion and check §8's finding from this side: the
    /// grid it leaves behind IS the input text. `overflow` is the shortest at this rev
    /// (54 frames); the bound is generous so a slower upstream build does not fail it.
    #[test]
    fn an_effect_runs_to_none_and_settles_on_the_input() {
        let input = "█▀\n▄█";
        let mut p = Producer::start(input, "overflow", 1, 60, None, None).unwrap();
        let mut steps = 0;
        while p.step() {
            steps += 1;
            assert!(steps < 5000, "overflow did not finish");
        }
        assert!(p.done && !p.step(), "done is sticky");
        assert!(steps > 0);
        assert_eq!(p.tick, steps);
        let mut cells = Vec::new();
        assert_eq!(p.walk(&mut cells), (2, 2));
        let syms: String = cells.iter().map(|c| char::from_u32(c.symbol).unwrap()).collect();
        assert_eq!(syms, "█▀▄█");
        // Colour is present on the settled text (the effect's final gradient), and it
        // is 8-bit sRGB as ttfx stores it — the consumer decodes it, not the producer.
        assert!(cells.iter().all(|c| c.sgr & SGR_HAS_FG != 0));
        assert!(cells.iter().all(|c| c.character_id < 4));
    }

    /// Two runs with one seed are one run: the fixture grid T2's harness wants.
    #[test]
    fn a_seed_makes_the_walk_deterministic() {
        let run = |seed| {
            let mut p = Producer::start("abc\ndef", "rain", seed, 60, None, None).unwrap();
            let mut grids = Vec::new();
            let mut cells = Vec::new();
            for _ in 0..40 {
                if !p.step() {
                    break;
                }
                p.walk(&mut cells);
                grids.push(cells.clone());
            }
            grids
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8), "a different seed must move something in 40 frames");
    }

    /// §7: a character between two cells carries the remainder its integer coordinate
    /// dropped, in ttfx's own frame (+x right, +y up — the ring's, so no flip); a
    /// character *placed* on a cell carries exactly zero. Driven through the real engine
    /// so the convention is measured, not assumed: the last character (`d`, input cell
    /// (2,1) — the highest id, so it wins any cell it shares; `a` moved onto `b` in the
    /// first draft and vanished from the walk) on a path up and to the right, one tick at
    /// 0.7 cells/tick, lands at (2.48, 1.26): remainder (+0.48, +0.26) against its rounded
    /// cell (2,1). The expected pair is computed inline from the motion, NOT through
    /// `sub_cell_offset`, so a swapped axis in the helper fails here.
    #[test]
    fn a_character_between_cells_carries_its_remainder_and_a_placed_one_carries_zero() {
        use ttfx::engine::ctx::NoopHooks;
        use ttfx::engine::terminal::{CharacterFilter, CharacterSort};
        use ttfx::utils::geometry::Coord;
        let mut ctx = EngineCtx::new(
            "ab\ncd",
            terminal_config(60, Some(20), Some(10)),
            Rng::seeded(0),
            Clock::virtual_with_frame_rate(60),
        )
        .unwrap();
        let ids = ctx.terminal.get_characters(
            &mut Rng::seeded(0),
            CharacterFilter::default(),
            CharacterSort::TopToBottomLeftToRight,
        );
        for &id in &ids {
            ctx.terminal.set_character_visibility(id, true);
        }
        let id = *ids.last().unwrap();
        let cid = ctx.terminal.arena[id.0 as usize].character_id;
        {
            let m = &mut ctx.terminal.arena[id.0 as usize].motion;
            m.new_path(0.7, None, None, 0, false, "diag").unwrap();
            m.paths.get_mut("diag").unwrap().new_waypoint(Coord::new(15, 8), None, "").unwrap();
        }
        ctx.activate_path(&mut NoopHooks, id, "diag");
        ctx.tick(&mut NoopHooks, id);
        let mut cells = Vec::new();
        let find = |cells: &[GlyphCell]| *cells.iter().find(|c| c.symbol != 0 && c.character_id == cid).expect("walked");
        walk_grid(&ctx.terminal, &mut cells);
        let cell = find(&cells);
        let m = &ctx.terminal.arena[id.0 as usize].motion;
        assert!(m.active_path.is_some() && cell.sgr & SGR_ACTIVE_PATH != 0);
        let expect_x = (m.current_pos.0 - m.current_coord.column as f64) as f32;
        let expect_y = (m.current_pos.1 - m.current_coord.row as f64) as f32;
        assert_eq!((cell.sub_x, cell.sub_y), (expect_x, expect_y), "the cell carries the motion's remainder, x as x and y as y");
        assert!(expect_x != expect_y && expect_x > 0.0 && expect_y > 0.0, "the fixture must tell the axes apart: {:?}", (expect_x, expect_y));
        assert!(cell.sub_x.abs() <= 0.5 && cell.sub_y.abs() <= 0.5);
        // A placement has no remainder: the cell it lands in encodes zero.
        ctx.terminal.arena[id.0 as usize].motion.set_coordinate(Coord::new(5, 5));
        walk_grid(&ctx.terminal, &mut cells);
        let placed = find(&cells);
        assert_eq!((placed.sub_x, placed.sub_y), (0.0, 0.0));
        // And the cell index is the flipped one: row 5 of 10, bottom-up, is ring row 5.
        let idx = cells.iter().position(|c| c.symbol != 0 && c.character_id == cid).unwrap();
        assert_eq!((idx % 20, idx / 20), (4, 5));
    }

    #[test]
    fn pick_next_never_repeats_when_it_has_a_choice() {
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut rng = Rng::seeded(3);
        let mut prev = pick_next(&mut rng, &names, None);
        for _ in 0..200 {
            let n = pick_next(&mut rng, &names, Some(&prev));
            assert_ne!(n, prev);
            prev = n;
        }
        let one = vec!["only".to_string()];
        assert_eq!(pick_next(&mut rng, &one, Some("only")), "only");
    }
}
