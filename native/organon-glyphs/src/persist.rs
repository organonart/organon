//! Phosphor persistence — the producer half of organon#217 T11 (`doc/pbr_text_engine.md`
//! §15, the tile the spec-sheet plate labels "PHOSPHOR PERSISTENCE").
//!
//! A CRT phosphor keeps emitting after the beam leaves it, decaying roughly
//! exponentially. `ttfx` has no such thing — a terminal cell is lit or it is not — and
//! that cut is one reason the first render read as a spreadsheet where the plates read
//! as a display. So it lives **here**, between the walk and the ring: [`Persistence::apply`]
//! takes the walked grid (the *source*) and rewrites it in place, keeping one phosphor
//! per cell. Nothing upstream (ttfx) or downstream (the ring's colour contract, the
//! world's decode) changes shape.
//!
//! **In linear light, always.** A ring colour is sRGB8 (§4: display-referred). The
//! phosphor's residual is kept as *linear* RGB, decays there — `glow *= e^(-dt/τ)` per
//! channel — and is re-encoded through [`linear_to_srgb8`] on the way out. Decaying the
//! encoded byte would make trails fall too fast; *skipping the decode* would make a
//! mid-grey trail come out **brighter** than its source (128 → 187), which is §4's gamma
//! bug arriving from the encode side. Both are pinned by test.
//!
//! **The rule: excitation is instant, decay is slow, and a phosphor cannot be un-lit by
//! a new colour.** A lit cell publishes `max(source, residual)` **per channel**: a
//! steadily lit cell is exactly its source (`max(s, s·k) = s`, so it publishes the byte
//! it arrived as — untouched, not re-encoded); a cell that goes from bright to dim shows
//! the bright residual fading *into* the dim source; a cell that changes hue at equal
//! brightness shows the old hue's residual under the new one, which is what a real
//! phosphor does (its emission is additive) without the runaway that a literal sum would
//! have — a constant source re-excited every tick would otherwise converge to
//! `s / (1 - k)` and blow through white. The alternative, "the source replaces", would
//! cut every bright→dim transition, which is most of what `decrypt`'s resolve *is*.
//!
//! **What a trail carries.** When the source goes dark and the residual is above
//! [`PERSIST_FLOOR`], the cell publishes the **last lit cell** — its symbol (the tile
//! shape is what fades), `bg`, SGR attributes, `layer`, `character_id`, sub-cell offset —
//! with `fg` replaced by the decayed residual and `SGR_PERSIST` set, `SGR_ACTIVE_PATH`
//! cleared (a trail does not move; `lower_grid` also never takes a trail as a slide's
//! origin). Below the floor the phosphor is spent and the cell reverts to whatever its
//! source is. A lit cell with **no** foreground colour (`colors: None`, the terminal's
//! default) leaves no trail: the colour it draws in is `GlyphLook::default_fg`, a look
//! constant of the renderer's that T3 is lifting onto the param chain, and the producer
//! must not bake a copy of it into the ring.
//!
//! **Time is the producer's published time, nominal.** The caller passes `dt`: the tick
//! period during motion, the heartbeat interval during the dwell, zero for the settle
//! publish (it is the same instant as the last motion frame). Nominal rather than
//! measured so that a seed reproduces a run byte for byte, and published rather than
//! effect time because persistence is a property of the *display*: `--tick-hz` below
//! `--fps` slows the effect, not the phosphor. The phosphors outlive an effect — the
//! settled text of one fades under the opening of the next — and reset only when the
//! grid changes size.
//!
//! **Off is a no-op**, not a decay of zero: with `τ == 0` `apply` returns before it
//! touches a byte or allocates a phosphor, which is what makes `--persist-ms 0` (the
//! default, invariant #4) byte-identical to a producer that predates this module.

use organon_core::glyph_ring::{
    linear_rgb, pack_linear_rgb, tile_for, GlyphCell, SGR_ACTIVE_PATH, SGR_HAS_FG, SGR_PERSIST,
};

/// The floor, in linear light: a trail whose every channel is below this is cleared. At
/// `1e-3` the re-encoded byte is 3/255 — invisible on an emissive tile at any gain the
/// look uses, and low enough that the cut to black is not a step. It is not lower
/// because a trail keeps the ring's `generation` moving (the payload changes every tick
/// it decays), which restarts T5's path-trace accumulation; the tail from 3/255 down to
/// the encoder's own floor of 1/255 would cost another ~2τ of that for nothing visible.
/// A trail from full white lasts `τ·ln(1/1e-3) ≈ 6.9τ`; the dimmer the source, the
/// shorter.
pub const PERSIST_FLOOR: f32 = 1.0e-3;

/// One cell's phosphor: the residual emission and the cell it came from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Phosphor {
    /// Residual emission, linear RGB. All zero = spent.
    glow: [f32; 3],
    /// The last lit source cell — what a trail publishes, minus its colour.
    last: GlyphCell,
}

/// Per-cell phosphor state over a grid. Create once per producer run and call
/// [`apply`](Self::apply) on every walked grid before it is published.
#[derive(Clone, Debug)]
pub struct Persistence {
    tau_s: f32,
    dims: (u32, u32),
    cells: Vec<Phosphor>,
}

impl Persistence {
    /// `persist_ms` is the decay time constant τ in milliseconds. Zero, negative or
    /// non-finite = off.
    pub fn new(persist_ms: f64) -> Persistence {
        let tau_s = if persist_ms.is_finite() && persist_ms > 0.0 { (persist_ms / 1000.0) as f32 } else { 0.0 };
        Persistence { tau_s, dims: (0, 0), cells: Vec::new() }
    }

    /// The no-op persistence: `apply` touches nothing.
    pub fn off() -> Persistence {
        Persistence::new(0.0)
    }

    pub fn enabled(&self) -> bool {
        self.tau_s > 0.0
    }

    /// τ in seconds (0 when off).
    pub fn tau_s(&self) -> f32 {
        self.tau_s
    }

    /// Advance the phosphors by `dt_s` seconds and rewrite `cells` (the walked source,
    /// `cols × rows`, top-down) into what the ring should carry. See the module doc for
    /// the rule. A grid of another size than the last call resets every phosphor.
    pub fn apply(&mut self, cells: &mut [GlyphCell], cols: u32, rows: u32, dt_s: f32) {
        if !self.enabled() {
            return;
        }
        let n = cols as usize * rows as usize;
        debug_assert_eq!(n, cells.len(), "grid is {cols}x{rows} but {} cells were given", cells.len());
        if self.dims != (cols, rows) || self.cells.len() != n {
            self.dims = (cols, rows);
            self.cells.clear();
            self.cells.resize(n, Phosphor::default());
        }
        let dt = if dt_s.is_finite() { dt_s.max(0.0) } else { 0.0 };
        let decay = (-dt / self.tau_s).exp();
        for (cell, ph) in cells.iter_mut().zip(self.cells.iter_mut()) {
            for g in &mut ph.glow {
                *g *= decay;
            }
            // Lit = draws a tile AND has a colour of its own. A space with a colour
            // lights nothing; a block with the terminal's default colour has nothing
            // this side of the ring can decay (module doc).
            let lit = cell.sgr & SGR_HAS_FG != 0 && tile_for(cell.symbol).is_some();
            if lit {
                let src = linear_rgb(cell.fg);
                let out = [src[0].max(ph.glow[0]), src[1].max(ph.glow[1]), src[2].max(ph.glow[2])];
                ph.glow = out;
                ph.last = *cell;
                // Only re-encode when the residual actually won a channel: a steadily
                // lit cell publishes the byte it arrived as, never a round-tripped one.
                if out != src {
                    cell.fg = pack_linear_rgb(out);
                }
            } else if ph.glow.iter().any(|&g| g >= PERSIST_FLOOR) {
                let mut trail = ph.last;
                trail.fg = pack_linear_rgb(ph.glow);
                trail.sgr = (trail.sgr & !SGR_ACTIVE_PATH) | SGR_HAS_FG | SGR_PERSIST;
                *cell = trail;
            } else if ph.glow != [0.0; 3] {
                // Fell below the floor: spent. The cell stays whatever its source is.
                *ph = Phosphor::default();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Producer;
    use organon_core::glyph_ring::{
        linear_to_srgb8, pack_rgb, srgb8_to_linear, unpack_rgb, SGR_BOLD, SGR_HAS_BG,
    };

    fn lit(sym: char, r: u8, g: u8, b: u8) -> GlyphCell {
        GlyphCell { symbol: sym as u32, fg: pack_rgb(r, g, b), sgr: SGR_HAS_FG, character_id: 7, ..Default::default() }
    }

    fn bytes(c: &GlyphCell) -> [u8; 3] {
        unpack_rgb(c.fg)
    }

    /// Invariant #4, measured over a real effect: with τ = 0 every published frame —
    /// motion, settle and dwell heartbeats — is the walk, byte for byte, and no state
    /// is even allocated. The same fixture with τ > 0 must leave real trails (cells
    /// flagged `SGR_PERSIST` where the walk is dark), or this test would pass against a
    /// persistence that never did anything. ⚠️ Not every effect does: measured at this
    /// rev over a 24×2 fixture at 120 fps, `decrypt` (752 frames), `wipe`, `expand`,
    /// `slice` and `middleout` never let a lit cell go dark, so under them the only
    /// difference persistence makes is the bright→dim rule — found when a mutation of
    /// that rule failed *this* test on `overflow`. `rain` drops every character from the
    /// top and leaves the cells it fell through dark: 55 frames, 301 trail cells.
    #[test]
    fn off_is_byte_identical_to_the_walk_over_a_whole_effect_and_its_dwell() {
        let mut p = Producer::start("█▀▄▀█▄▀█▄▀█▄▀█▄▀█▄▀█▄▀█\n▄█▀█▄▀█▄▀█▄▀█▄▀█▄▀█▄▀█▄▀", "rain", 1, 120, None, None).unwrap();
        let mut off = Persistence::new(0.0);
        let mut on = Persistence::new(300.0);
        let (mut raw, mut a, mut b) = (Vec::new(), Vec::new(), Vec::new());
        let (mut frames, mut differed, mut trails) = (0, 0, 0);
        let (cols, rows) = loop {
            let moved = p.step();
            let (cols, rows) = p.walk(&mut raw);
            let dt = if moved { 1.0 / 120.0 } else { 0.0 }; // the settle publish: same instant
            a.clone_from(&raw);
            off.apply(&mut a, cols, rows, dt);
            assert_eq!(a, raw, "frame {frames}: --persist-ms 0 must publish the walk byte for byte");
            b.clone_from(&raw);
            on.apply(&mut b, cols, rows, dt);
            differed += (b != raw) as u32;
            trails += b.iter().zip(&raw).filter(|(t, r)| t.sgr & SGR_PERSIST != 0 && tile_for(r.symbol).is_none()).count();
            frames += 1;
            if !moved {
                break (cols, rows);
            }
        };
        for beat in 0..16 {
            a.clone_from(&raw);
            off.apply(&mut a, cols, rows, 0.25);
            assert_eq!(a, raw, "heartbeat {beat}: the dwell republishes the walk byte for byte");
        }
        assert!(frames > 10, "{frames}");
        assert!(differed > 0 && trails > 0, "the fixture must exercise persistence: with τ = 300 ms some frame must differ ({differed} did) and some dark cell must carry a trail ({trails} did), or this test proves nothing");
        assert!(off.cells.is_empty(), "off allocates no phosphors");
        assert!(!off.enabled() && on.enabled() && !on.cells.is_empty());
        assert!(!Persistence::new(-5.0).enabled() && !Persistence::new(f64::NAN).enabled());
    }

    /// A cell lit at one colour, tick after tick, publishes its source untouched — not
    /// a round-tripped copy of it, the byte it arrived as.
    #[test]
    fn a_steady_source_is_published_untouched() {
        let mut ps = Persistence::new(300.0);
        let src = [lit('█', 77, 128, 201)];
        for tick in 0..120 {
            let mut cells = src;
            ps.apply(&mut cells, 1, 1, 1.0 / 60.0);
            assert_eq!(cells, src, "tick {tick}");
        }
    }

    /// The trail of a full-white cell is `e^-k` after `k` time constants — in LINEAR
    /// light, read back through the ring's own decode. Decaying the encoded byte instead
    /// would put the first sample at 255/e = 93 where linear puts it at 163.
    #[test]
    fn decay_is_exponential_in_linear_light() {
        let mut ps = Persistence::new(1000.0); // τ = 1 s
        let src = [lit('█', 255, 255, 255)];
        let mut cells = src;
        ps.apply(&mut cells, 1, 1, 0.5);
        assert_eq!(cells, src, "a lit cell with no residual is its source");
        let mut expect = 1.0f32;
        for k in 1..=5 {
            let mut cells = [GlyphCell::default()];
            ps.apply(&mut cells, 1, 1, 1.0); // one τ
            expect /= std::f32::consts::E;
            let got = bytes(&cells[0]);
            let want = linear_to_srgb8(expect);
            assert!(
                (got[0] as i32 - want as i32).abs() <= 1 && got[0] == got[1] && got[1] == got[2],
                "after {k}τ the trail is e^-{k} = {expect:.4} in LINEAR light, i.e. byte {want}; published {got:?} (linear {:.4})",
                srgb8_to_linear(got[0])
            );
            assert_ne!(cells[0].sgr & SGR_PERSIST, 0, "{k}τ: still a trail");
        }
    }

    /// §4's gamma bug from the encode side: a mid-grey source's trail must never be
    /// brighter than the source. With no time passed the trail IS the source byte.
    #[test]
    fn a_mid_grey_trail_never_brightens() {
        let mut ps = Persistence::new(300.0);
        let mut cells = [lit('█', 128, 128, 128)];
        ps.apply(&mut cells, 1, 1, 0.0);
        let mut cells = [GlyphCell::default()];
        ps.apply(&mut cells, 1, 1, 0.0);
        let b = bytes(&cells[0]);
        assert!(
            b[0] <= 128,
            "a mid-grey trail brightened to {} > 128: the source byte was treated as linear and re-encoded — §4's gamma bug",
            b[0]
        );
        assert_eq!(b, [128, 128, 128], "with no time passed the trail is the source byte, exactly");
        assert_ne!(cells[0].sgr & SGR_PERSIST, 0);
        let mut cells = [GlyphCell::default()];
        ps.apply(&mut cells, 1, 1, 0.05);
        assert!(bytes(&cells[0])[0] < 128, "any time at all makes it darker: {:?}", bytes(&cells[0]));
    }

    /// A trail is the last lit cell — symbol, attributes, position, identity — with its
    /// colour decaying, the trail bit set and the path bit clear; it crosses the floor
    /// after `τ·ln(peak/floor)` and the cell reverts to its (dark) source.
    #[test]
    fn a_trail_keeps_its_symbol_carries_the_flag_and_clears_below_the_floor() {
        let (tau, dt) = (0.3f32, 1.0f32 / 60.0);
        let mut ps = Persistence::new(300.0);
        let mut src = lit('▀', 0, 200, 90);
        src.sgr |= SGR_ACTIVE_PATH | SGR_BOLD | SGR_HAS_BG;
        src.bg = pack_rgb(9, 9, 9);
        src.sub_x = 0.25;
        src.sub_y = -0.5;
        src.layer = 2;
        let mut cells = [src];
        ps.apply(&mut cells, 1, 1, dt);
        let (mut ticks, mut last_trail): (i32, Option<[u8; 3]>) = (0, None);
        loop {
            let mut cells = [GlyphCell::default()];
            ps.apply(&mut cells, 1, 1, dt);
            ticks += 1;
            if cells[0] == GlyphCell::default() {
                break;
            }
            let t = cells[0];
            assert_eq!(t.symbol, '▀' as u32, "the tile shape is what fades");
            assert!(t.sgr & SGR_PERSIST != 0 && t.sgr & SGR_HAS_FG != 0, "a trail is flagged and coloured: {:#x}", t.sgr);
            assert_eq!(t.sgr & SGR_ACTIVE_PATH, 0, "a trail does not move");
            assert!(t.sgr & SGR_BOLD != 0 && t.sgr & SGR_HAS_BG != 0, "everything but the colour and the path bit is the last lit cell's");
            assert_eq!((t.sub_x, t.sub_y, t.layer, t.bg, t.character_id), (0.25, -0.5, 2, pack_rgb(9, 9, 9), 7));
            let b = bytes(&t);
            assert_eq!(b[0], 0, "a channel that was dark stays dark");
            if let Some(prev) = last_trail {
                assert!(b[1] <= prev[1] && b[2] <= prev[2], "monotone: {prev:?} -> {b:?}");
            }
            last_trail = Some(b);
            assert!(ticks < 10_000, "the trail never cleared");
        }
        let peak = srgb8_to_linear(200);
        let expect = (tau * (peak / PERSIST_FLOOR).ln() / dt).floor() as i32 + 1;
        assert!((ticks - expect).abs() <= 2, "cleared after {ticks} ticks, expected {expect} = floor(τ·ln(peak/floor)/dt) + 1");
        let last = last_trail.expect("there was a trail");
        assert!(last.iter().all(|&b| b <= 4), "at this dt/τ the cut at the floor is from at most 4/255 to 0: {last:?}");
        // Spent means spent: more dark ticks stay dark, and re-lighting starts fresh.
        let mut cells = [GlyphCell::default()];
        ps.apply(&mut cells, 1, 1, 10.0);
        assert_eq!(cells[0], GlyphCell::default());
        assert!(ps.cells[0].glow == [0.0; 3] && ps.cells[0].last == GlyphCell::default());
    }

    /// The rule for a lit source over a residual: per-channel max. A dim source under a
    /// bright residual shows the residual fading into it and is NOT a trail (its symbol
    /// is the source's, no flag); once the residual falls under the source the published
    /// cell is the source, byte for byte. A new hue at full brightness sits over the old
    /// hue's residual — excitation is instant, decay is slow.
    #[test]
    fn a_lit_source_over_a_trail_is_the_brighter_per_channel() {
        let (tau, dt) = (0.3f32, 0.05f32);
        let mut ps = Persistence::new(300.0);
        let mut cells = [lit('█', 255, 0, 0)];
        ps.apply(&mut cells, 1, 1, dt);
        let dim = lit('▄', 40, 0, 0);
        let mut cells = [dim];
        ps.apply(&mut cells, 1, 1, dt);
        let b = bytes(&cells[0]);
        assert!(b[0] > 40 && b[0] < 255, "the residual outshines a dim source: {b:?}");
        assert_eq!(b[0], linear_to_srgb8((-dt / tau).exp()), "…by exactly its own decayed value");
        assert_eq!(cells[0].symbol, '▄' as u32, "…but the cell IS lit: the symbol is the source's");
        assert_eq!(cells[0].sgr & SGR_PERSIST, 0, "…and it is not a trail");
        let mut settled = None;
        for i in 0..400 {
            let mut cells = [dim];
            ps.apply(&mut cells, 1, 1, dt);
            if cells[0] == dim {
                settled = Some(i);
                break;
            }
            assert!(bytes(&cells[0])[0] >= 40, "never below the source: {:?}", bytes(&cells[0]));
        }
        let i = settled.expect("the residual must fall into the source");
        // Residual e^-(k·dt/τ) ≤ lin(40) ⇔ k ≥ τ·ln(1/lin(40))/dt; one tick was spent above.
        let expect = (tau * (1.0 / srgb8_to_linear(40)).ln() / dt).ceil() as i32 - 1;
        assert!((i as i32 - expect).abs() <= 2, "the source takes over after {i} ticks, expected about {expect}");
        // Hue: full green over a red residual keeps the red under it.
        let mut ps = Persistence::new(300.0);
        let mut cells = [lit('█', 255, 0, 0)];
        ps.apply(&mut cells, 1, 1, dt);
        let mut cells = [lit('█', 0, 255, 0)];
        ps.apply(&mut cells, 1, 1, dt);
        let b = bytes(&cells[0]);
        assert!(b[0] > 0 && b[1] == 255 && b[2] == 0, "red residual under full green: {b:?}");
    }

    /// No colour, no trail: a block in the terminal's default colour and a coloured
    /// space both light nothing this side of the ring.
    #[test]
    fn a_cell_without_a_colour_of_its_own_leaves_no_trail() {
        let mut ps = Persistence::new(300.0);
        let plain = GlyphCell { symbol: '█' as u32, character_id: 3, ..Default::default() };
        let space = GlyphCell { symbol: ' ' as u32, ..lit('█', 255, 255, 255) };
        let mut cells = [plain, space];
        ps.apply(&mut cells, 2, 1, 0.01);
        assert_eq!(cells, [plain, space], "published as they are");
        let mut cells = [GlyphCell::default(); 2];
        ps.apply(&mut cells, 2, 1, 0.01);
        assert_eq!(cells, [GlyphCell::default(); 2], "and nothing lingers");
    }

    /// A grid of another size cannot be matched cell for cell: the phosphors reset.
    #[test]
    fn a_dimension_change_resets_the_phosphors() {
        let mut ps = Persistence::new(300.0);
        let mut cells = [lit('█', 255, 255, 255)];
        ps.apply(&mut cells, 1, 1, 0.01);
        let mut cells = [GlyphCell::default(); 2];
        ps.apply(&mut cells, 2, 1, 0.01);
        assert_eq!(cells, [GlyphCell::default(); 2]);
        // …and the same size keeps them: the settled text of one effect fades under the
        // next (the binary keeps one `Persistence` across effects).
        let mut cells = [lit('█', 255, 255, 255), GlyphCell::default()];
        ps.apply(&mut cells, 2, 1, 0.01);
        let mut cells = [GlyphCell::default(); 2];
        ps.apply(&mut cells, 2, 1, 0.01);
        assert_ne!(cells[0].sgr & SGR_PERSIST, 0);
        assert_eq!(cells[1], GlyphCell::default());
    }
}
