//! The **glyph ring** — a cell grid carried from a text-effect producer to the renderer
//! (`doc/pbr_text_engine.md` §6, tier T1 of organon#217).
//!
//! A SEPARATE memory-mapped channel from `Shared`, on the `mind_ring` / `audio_ring` /
//! frame-mirror precedent (`ipc::glyph_ring_path`): a terminal-shaped grid of cells at
//! the producer's own cadence is neither control-rate nor small, and `Shared`'s byte
//! offsets are load-bearing across every saved DAW session. Nothing here touches
//! `Shared`, and no `LAYOUT_VERSION` moves — that is the whole point of §6.
//!
//! **Who writes, who reads.** The writer is `organon-glyphs` (the producer crate that
//! links `ttfx` and ticks an effect under a virtual clock); the reader is the world
//! (`organon-world/src/world.rs`), which turns each non-empty cell into an instanced,
//! bevelled, emissive tile. Both are separate binaries indexing one mmap by byte offset,
//! so — exactly as `mind_ring.rs` learned — a layout disagreement does not fail, it
//! *renders wrong*. Two guards: the header carries `cell_bytes` (the writer's
//! `size_of::<GlyphCell>()`) and `layout_version`, and the reader refuses a ring that
//! disagrees on either.
//!
//! **Double buffer with a lap guard, not a slot ring.** A grid is up to a megabyte and
//! the reader wants only the newest one, so there are two slots and the writer always
//! fills the one the reader is *not* looking at: slot `(write_seq - 1) % 2` is the
//! latest, the other is the writer's scratch. The reader reads `write_seq`, copies that
//! slot, and reads `write_seq` again; if it advanced by **two or more** the writer has
//! come round to the slot being copied and the copy is retried. An advance of exactly one
//! is fine — that write went to the other slot. This is a seqlock in spirit (readers never
//! block the writer, a torn read is detected and retried rather than prevented) with the
//! one-writer/one-slot rule doing the work a per-cell sequence number would otherwise do.
//! A slot ring (`MR_SLOTS = 4`) was the alternative; at this payload size two slots is the
//! right depth, and the lap guard is what makes two enough.
//!
//! **Orientation — read this before indexing cells.** `ttfx` (and TTE before it)
//! numbers rows from the BOTTOM: `Coord` is 1-based with row 1 at the bottom of the
//! canvas. The ring stores rows **top-down**, the way a text file reads: cell `(c, r)`
//! is at `cells[r * cols + c]` with `r == 0` the TOP line. The producer flips once
//! (`ring_row = rows - ttfx_row`); the consumer never sees a bottom-up row. A wrong flip
//! is invisible on a symmetric logo, so the tests here use an asymmetric fixture.
//!
//! **The cell aspect travels in the header.** TTE's cell is 2:1 (`geometry.rs` doubles
//! row deltas in every length and doubles x on every circle — §7), so every ring and
//! spiral in the effect set is authored for a cell twice as tall as it is wide. Square
//! tiles would turn them into ellipses. The renderer reads `cell_aspect` rather than
//! guessing.
//!
//! **Per cell the ring carries more than a terminal would** (§6.1): symbol, fg, bg and
//! SGR flags are the terminal's atom; `layer`, `character_id` and `active_path` are what
//! the library knows and the terminal forgets. `active_path` is the slide-versus-cut
//! signal — the consumer interpolates `previous → current` only while it is set (§7).
//! `sub_x` / `sub_y` carry the pre-rounded sub-cell offset §7 describes — the
//! remainder the effect's integer coordinate dropped — so a tile sits where the path
//! put the character, not on the nearest cell. They were reserved from day one and
//! filled by the producer once ttfx exposed `Motion.current_pos` (organonart/ttfx PR);
//! the encoding is on the fields. No `layout_version` move: the bytes were already in
//! the cell and already zero, and a reader that ignores them sees exactly what it saw.
//!
//! **A trail is a cell, flagged** (T11, §15). Phosphor persistence lives in the
//! producer: when a lit cell's source goes dark it keeps publishing the last lit cell
//! with its colour decayed — in linear light, re-encoded to sRGB8, because **the ring's
//! colour contract does not change** — and `SGR_PERSIST` set so the renderer can tell a
//! trail from a lit cell. The header does not carry the time constant: the colour
//! arrives already decayed, and the flag is all a reader needs. A bit in an existing
//! word, so again no `layout_version` move — a reader that predates the bit draws a
//! dimmer tile, which is the right picture, just without knowing why.
//!
//! **Every cell is a tile** (T9, §15 — the lowering half; the shading half is
//! `cube.wgsl`'s clearcoat and emission profile). The spec-sheet plate shows a dark cell
//! as a dark, glass-capped tile that reflects the room, a quarter as proud as a lit
//! glyph; T1 drew a dark cell as nothing, i.e. bare backplane. Behind
//! [`LowerOptions::dark_tiles`] — **off by default and byte-identical off** (invariant
//! #4) — [`lower_grid_with`] lowers a symbol-less cell as a full-cell tile at the `░`
//! depth with zero emission, so the faceplate's environment sheen is all it shows.
//! The switch is a lowering option rather than a `GlyphLook` field because the world
//! builds `GlyphLook` by full struct literal (`world.rs::glyph_look_from`), and a new
//! field there would not compile until the wire lands; an option with a `Default` lets
//! this land first and the wire follow.
//!
//! **The blend is a clock, and the clock lives here** (T12's finding, §7). The world
//! holds two grids and draws a path character between them; how far between is
//! [`BlendClock`]'s answer, computed from what the ring already carries — `tick` is the
//! producer's clock, `Δtick / tick_hz` the span of the pair — plus the world's own
//! frame interval as a lead, because a frame built now is shown one interval later.
//! The old clock measured from the read over a fixed tick and evaluated at build time,
//! which at a producer faster than the display put every frame at `blend ≈ 0`: the
//! grid read one frame earlier, two ticks behind, never between. A heartbeat (same
//! `tick`) replaces the picture without touching the clock. Nothing new travels on the
//! wire and `layout_version` does not move; the section above the lowering has the
//! arithmetic and the reason a publish stamp alone would not have been enough.

use bytemuck::{Pod, Zeroable};
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

/// "GLYP" — wrong-file / torn-header guard stamped in the ring header.
pub const GLYPH_RING_SIGNATURE: u32 = 0x47_4C_59_50;
/// Bump when `GlyphCell` / `GlyphFrame` / the header change shape. A reader refuses a
/// ring whose writer disagrees, because a stale writer beside a fresh reader would not
/// crash — it would draw plausible garbage.
pub const GLYPH_LAYOUT_VERSION: u32 = 1;
/// Slot capacity in cells. 256 × 128 covers a 4K terminal at a small font; Omarchy's
/// screensaver is 100 × 30. A grid larger than this is refused by the writer (never
/// truncated silently — a clipped logo looks like a producer bug on the wrong side).
pub const GR_MAX_CELLS: usize = 256 * 128;
/// Two slots: the latest, and the writer's scratch. See the module doc for why two is
/// enough here and why it is not a `MR_SLOTS`-style ring.
pub const GR_SLOTS: usize = 2;
/// Bytes of the NUL-padded effect name carried per frame (debugging / the overlay).
pub const GR_NAME_LEN: usize = 32;

/// The cell aspect of a `ttfx` terminal cell — height over width. §7: authored 2:1.
pub const TTFX_CELL_ASPECT: f32 = 2.0;

// ── `GlyphCell.sgr` bits ────────────────────────────────────────────────────────
// The eight SGR attributes `ttfx`'s `CharacterVisual` carries, in its field order, then
// the presence bits the `Option`s collapse to, then the library-only signal.
pub const SGR_BOLD: u32 = 1 << 0;
pub const SGR_DIM: u32 = 1 << 1;
pub const SGR_ITALIC: u32 = 1 << 2;
pub const SGR_UNDERLINE: u32 = 1 << 3;
pub const SGR_BLINK: u32 = 1 << 4;
pub const SGR_REVERSE: u32 = 1 << 5;
pub const SGR_HIDDEN: u32 = 1 << 6;
pub const SGR_STRIKE: u32 = 1 << 7;
/// `fg` holds a colour (the visual's `colors.fg_color` was `Some`).
pub const SGR_HAS_FG: u32 = 1 << 8;
/// `bg` holds a colour.
pub const SGR_HAS_BG: u32 = 1 << 9;
/// `motion.active_path` was `Some` — the character is sliding along a path, so the
/// consumer may interpolate its previous → current cell (§7). Clear = it was placed by
/// `set_coordinate`, i.e. a teleport, and interpolating would invent motion.
pub const SGR_ACTIVE_PATH: u32 = 1 << 10;
/// **This cell is a phosphor trail, not a lit cell** (organon#217 T11, §15). The
/// producer's persistence pass kept the last lit cell here after its source went dark:
/// `symbol`, `bg`, the SGR attributes, `layer`, `character_id` and the sub-cell offset
/// are the last lit cell's, and `fg` is that cell's colour **already decayed** in linear
/// light and re-encoded to sRGB8 — so a reader that knows nothing of this bit draws a
/// correctly dimmer tile, and one that does can draw it without a faceplate highlight
/// (T9). Never set together with `SGR_ACTIVE_PATH`: a trail does not move, and
/// `lower_grid` never takes a trail as a slide's origin. Cleared on the tick the trail
/// falls below the producer's floor, when the cell reverts to whatever its source is.
pub const SGR_PERSIST: u32 = 1 << 11;

// ── `GlyphFrame.flags` bits ─────────────────────────────────────────────────────
/// The effect has returned `None`: this grid is the settled text, held for the dwell
/// (§8). The consumer may treat it as a still (T5 converges the path tracer on it).
pub const FRAME_SETTLED: u32 = 1 << 0;

/// One cell of the grid. 32 bytes, `repr(C)`, no padding — the offsets are pinned by
/// test because the writer and reader are separate binaries.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GlyphCell {
    /// Unicode scalar value of the painted symbol; `0` = empty cell (nothing drawn).
    pub symbol: u32,
    /// Foreground, `0x00RRGGBB`, **sRGB-encoded** 8-bit — exactly what `ttfx` stores.
    /// Decode with [`srgb8_to_linear`] before it becomes emission (§4). Meaningful only
    /// when `sgr & SGR_HAS_FG`.
    pub fg: u32,
    /// Background, same encoding; meaningful only when `sgr & SGR_HAS_BG`.
    pub bg: u32,
    /// `SGR_*` bits.
    pub sgr: u32,
    /// `EffectCharacter.layer` — the painter's-order key, kept so a consumer can tell
    /// foreground effects from the text they pass over.
    pub layer: i32,
    /// `EffectCharacter.character_id` — the identity a cell's contents came from. This is
    /// what lets the consumer find *where this character was last frame* and slide it.
    pub character_id: u32,
    /// Sub-cell offset (§7): where the character *exactly* is, relative to this cell's
    /// centre, in cell units — `+x` right, `+y` up, each axis in `-0.5..=0.5`. The
    /// producer writes `Motion.current_pos - Motion.current_coord` (ttfx's pre-rounded
    /// path point minus its banker's rounding; `f64` narrowed to `f32`, the only loss).
    /// A character placed by `set_coordinate` — a cut — carries exactly `0.0, 0.0`, and
    /// so does every cell from a producer that predates the field, which is why this
    /// was reserved-to-defined with no `layout_version` move. ⚠️ Same sign as ttfx's
    /// own axes: the *row index* is flipped top-down by the producer, the remainder is
    /// not — `+y` is up on both sides of the ring.
    pub sub_x: f32,
    pub sub_y: f32,
}

/// Per-slot frame header, followed in the slot by `GR_MAX_CELLS` cells (of which the
/// first `cols * rows` are meaningful). 64 bytes, `repr(C)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GlyphFrame {
    /// The `write_seq` this slot was published under (the torn-read guard).
    pub seq: u32,
    pub cols: u32,
    pub rows: u32,
    /// Effect frame index, reset when a new effect starts — and **the producer's
    /// clock**: `tick / tick_hz` is this grid's publish time in producer seconds within
    /// its `epoch`, nominal rather than measured (the producer paces by a drift-free
    /// deadline and a seed reproduces a run — T11 keeps its phosphor time nominal for the
    /// same reason). The settle publish and every dwell heartbeat republish at the
    /// **same** `tick`, which is how a reader tells a heartbeat from a tick without a
    /// wall stamp (see [`classify_arrival`]). No wall time travels on the wire: the
    /// world needs the *span* between two grids, never the producer's absolute clock.
    pub tick: u32,
    /// Bumps when the cell payload **differs** from the previously published grid. A
    /// dwell republish (heartbeat) keeps it — that is the counter T5 hangs `pt_content`
    /// on, so accumulation restarts when the glyphs move and only then.
    pub generation: u32,
    /// Bumps per effect run (each `motion → settle → dwell → next` cycle).
    pub epoch: u32,
    /// `FRAME_*` bits.
    pub flags: u32,
    pub _pad: u32,
    /// NUL-padded effect name (`beams`, `rain`, …); see [`frame_name`].
    pub effect: [u8; GR_NAME_LEN],
}

impl Default for GlyphFrame {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// The ring header. 32 bytes, `repr(C)`, at file offset 0; the two slots follow.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GlyphRingHeader {
    pub signature: u32,
    pub layout_version: u32,
    /// `size_of::<GlyphCell>()` as the writer understood it — the layout guard.
    pub cell_bytes: u32,
    /// Slot capacity as the writer understood it (`GR_MAX_CELLS`).
    pub max_cells: u32,
    /// Height / width of one cell. `TTFX_CELL_ASPECT` from the ttfx producer.
    pub cell_aspect: f32,
    /// The producer's tick rate — how often a new `tick` lands. The consumer's
    /// interpolation window for `SGR_ACTIVE_PATH` cells is `1 / tick_hz`.
    pub tick_hz: f32,
    /// Frames ever published; the latest slot is `(write_seq - 1) % GR_SLOTS`.
    pub write_seq: u32,
    pub _pad: u32,
}

impl Default for GlyphRingHeader {
    fn default() -> Self {
        GlyphRingHeader {
            signature: GLYPH_RING_SIGNATURE,
            layout_version: GLYPH_LAYOUT_VERSION,
            cell_bytes: std::mem::size_of::<GlyphCell>() as u32,
            max_cells: GR_MAX_CELLS as u32,
            cell_aspect: TTFX_CELL_ASPECT,
            tick_hz: 60.0,
            write_seq: 0,
            _pad: 0,
        }
    }
}

const HDR_BYTES: usize = std::mem::size_of::<GlyphRingHeader>();
const FRAME_BYTES: usize = std::mem::size_of::<GlyphFrame>();
const CELL_BYTES: usize = std::mem::size_of::<GlyphCell>();
const SLOT_BYTES: usize = FRAME_BYTES + GR_MAX_CELLS * CELL_BYTES;
/// Total ring file size.
pub const GR_FILE_BYTES: usize = HDR_BYTES + GR_SLOTS * SLOT_BYTES;

fn slot_offset(slot: usize) -> usize {
    HDR_BYTES + slot * SLOT_BYTES
}

/// Write a NUL-padded effect name into a frame (truncated on a char boundary).
pub fn set_frame_name(f: &mut GlyphFrame, name: &str) {
    f.effect = [0; GR_NAME_LEN];
    let mut n = name.len().min(GR_NAME_LEN);
    while n > 0 && !name.is_char_boundary(n) {
        n -= 1;
    }
    f.effect[..n].copy_from_slice(&name.as_bytes()[..n]);
}

/// The effect name a frame carries.
pub fn frame_name(f: &GlyphFrame) -> String {
    let end = f.effect.iter().position(|&b| b == 0).unwrap_or(GR_NAME_LEN);
    String::from_utf8_lossy(&f.effect[..end]).into_owned()
}

/// Pack an sRGB8 triple the way the ring stores it.
pub fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Unpack a ring colour to its sRGB8 triple.
pub fn unpack_rgb(c: u32) -> [u8; 3] {
    [((c >> 16) & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, (c & 0xFF) as u8]
}

/// sRGB-encoded 8-bit → linear, the IEC 61966-2-1 transfer. §4: a TTE colour is
/// display-referred and **must** be decoded before it becomes emission — skipping this
/// makes mid-tones ~2× too bright and bends every gradient, and it looks "fine" at a
/// glance, which is what makes it the classic bug.
pub fn srgb8_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A packed ring colour → linear RGB.
pub fn linear_rgb(c: u32) -> [f32; 3] {
    let [r, g, b] = unpack_rgb(c);
    [srgb8_to_linear(r), srgb8_to_linear(g), srgb8_to_linear(b)]
}

/// Linear → sRGB-encoded 8-bit, the inverse of [`srgb8_to_linear`], rounded to nearest
/// and clamped. **The ring carries sRGB8, always** — a producer that works in linear
/// light (T11's persistence decays there, because that is where a phosphor decays) must
/// come back through this before it publishes. Pinned to round-trip every one of the 256
/// codes exactly, so a value that passed through linear untouched publishes the byte it
/// arrived as.
pub fn linear_to_srgb8(v: f32) -> u8 {
    let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    let c = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Linear RGB → the packed sRGB8 the ring stores.
pub fn pack_linear_rgb(rgb: [f32; 3]) -> u32 {
    pack_rgb(linear_to_srgb8(rgb[0]), linear_to_srgb8(rgb[1]), linear_to_srgb8(rgb[2]))
}

// ── Block-glyph geometry (§3) ───────────────────────────────────────────────────

/// How a cell's symbol becomes a tile. Every value is in **cell units** (§5.1: never
/// pixels): `x0..x1` across the cell's width, `y0..y1` up its height, both `0..=1`, and
/// `depth` the extrusion as a fraction of the full-block depth. `emission` scales the
/// cell's colour — `1.0` for the glyphs this tier renders faithfully, less for the
/// ones it stands in for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub depth: f32,
    pub emission: f32,
}

impl Tile {
    const fn rect(x0: f32, y0: f32, x1: f32, y1: f32, depth: f32) -> Tile {
        Tile { x0, y0, x1, y1, depth, emission: 1.0 }
    }
}

/// Map a symbol to its tile, or `None` for a cell that draws nothing (space, NUL, and
/// the zero-width / control range).
///
/// **Measured** (§3): Omarchy's logo is three glyphs — `█ ▀ ▄` — and the symbols the
/// 37 effects substitute in are overwhelmingly the same family. Every member of it is
/// an axis-aligned sub-cell rectangle: the eighth-blocks are offsets and extents, the
/// shade blocks `░ ▒ ▓` are a **coverage ramp** and become extrusion **depth** (25 / 50 /
/// 75 %) — the dithered fade that reads as stipple in a terminal becomes a height field
/// that catches light at each level, which is free and better than what it replaces.
///
/// **The unknown-symbol rule (T1, stated here so it is not discovered on screen):**
/// anything that is not a block/shade glyph — a letter, a digit, punctuation, a braille
/// or box-drawing character — renders as a **full block at reduced emission and depth**.
/// The effect's *colour and timing* still read (that is what carries the animation);
/// its *letterform* does not, and that is T7's job. Three-quadrant and diagonal
/// two-quadrant blocks (`▙ ▚ ▛ ▜ ▞ ▟`) are L- or checker-shaped, which one rectangle
/// cannot be; they render as a full block at their coverage depth, on the shade-block
/// argument.
pub fn tile_for(symbol: u32) -> Option<Tile> {
    // Depth of the unknown-symbol stand-in and its emission — a letter should read as
    // "a lit cell", not as a full-brightness block that hides which cells are text.
    const UNKNOWN: Tile = Tile { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0, depth: 0.5, emission: 0.45 };
    let t = match symbol {
        0 | 0x20 | 0xA0 | 0x3000 => return None, // NUL, space, NBSP, ideographic space
        c if c < 0x20 => return None,             // controls
        0x2588 => Tile::rect(0.0, 0.0, 1.0, 1.0, 1.0), // █ full block
        0x2580 => Tile::rect(0.0, 0.5, 1.0, 1.0, 1.0), // ▀ upper half
        0x2584 => Tile::rect(0.0, 0.0, 1.0, 0.5, 1.0), // ▄ lower half
        // ▁▂▃ ▅▆▇ — lower one-eighth … seven-eighths (▄ handled above, same rule)
        0x2581 => Tile::rect(0.0, 0.0, 1.0, 0.125, 1.0),
        0x2582 => Tile::rect(0.0, 0.0, 1.0, 0.25, 1.0),
        0x2583 => Tile::rect(0.0, 0.0, 1.0, 0.375, 1.0),
        0x2585 => Tile::rect(0.0, 0.0, 1.0, 0.625, 1.0),
        0x2586 => Tile::rect(0.0, 0.0, 1.0, 0.75, 1.0),
        0x2587 => Tile::rect(0.0, 0.0, 1.0, 0.875, 1.0),
        // ▉▊▋▌▍▎▏ — left seven-eighths … one-eighth
        0x2589 => Tile::rect(0.0, 0.0, 0.875, 1.0, 1.0),
        0x258A => Tile::rect(0.0, 0.0, 0.75, 1.0, 1.0),
        0x258B => Tile::rect(0.0, 0.0, 0.625, 1.0, 1.0),
        0x258C => Tile::rect(0.0, 0.0, 0.5, 1.0, 1.0),
        0x258D => Tile::rect(0.0, 0.0, 0.375, 1.0, 1.0),
        0x258E => Tile::rect(0.0, 0.0, 0.25, 1.0, 1.0),
        0x258F => Tile::rect(0.0, 0.0, 0.125, 1.0, 1.0),
        0x2590 => Tile::rect(0.5, 0.0, 1.0, 1.0, 1.0),   // ▐ right half
        0x2594 => Tile::rect(0.0, 0.875, 1.0, 1.0, 1.0), // ▔ upper one-eighth
        0x2595 => Tile::rect(0.875, 0.0, 1.0, 1.0, 1.0), // ▕ right one-eighth
        // ░ ▒ ▓ — the coverage ramp as extrusion depth
        0x2591 => Tile::rect(0.0, 0.0, 1.0, 1.0, 0.25),
        0x2592 => Tile::rect(0.0, 0.0, 1.0, 1.0, 0.5),
        0x2593 => Tile::rect(0.0, 0.0, 1.0, 1.0, 0.75),
        // Quadrants: ▖ lower-left, ▗ lower-right, ▘ upper-left, ▝ upper-right
        0x2596 => Tile::rect(0.0, 0.0, 0.5, 0.5, 1.0),
        0x2597 => Tile::rect(0.5, 0.0, 1.0, 0.5, 1.0),
        0x2598 => Tile::rect(0.0, 0.5, 0.5, 1.0, 1.0),
        0x259D => Tile::rect(0.5, 0.5, 1.0, 1.0, 1.0),
        // Non-rectangular quadrant combos → full block at coverage depth
        0x259A | 0x259E => Tile::rect(0.0, 0.0, 1.0, 1.0, 0.5), // ▚ ▞ two quadrants
        0x2599 | 0x259B | 0x259C | 0x259F => Tile::rect(0.0, 0.0, 1.0, 1.0, 0.75), // ▙ ▛ ▜ ▟
        _ => UNKNOWN,
    };
    Some(t)
}

// ── Writer ──────────────────────────────────────────────────────────────────────

/// Ring writer (the producer). Created once, then [`publish`](Self::publish) per tick.
pub struct GlyphRingWriter {
    map: memmap2::MmapMut,
    seq: u32,
    generation: u32,
    /// The last published payload, so `generation` bumps only on a real change.
    last: Vec<GlyphCell>,
    last_dims: (u32, u32),
}

impl GlyphRingWriter {
    /// Create/size the ring in this process's namespace (`ipc::glyph_ring_path`).
    pub fn create(cell_aspect: f32, tick_hz: f32) -> io::Result<GlyphRingWriter> {
        Self::create_at(&crate::ipc::glyph_ring_path(), cell_aspect, tick_hz)
    }

    /// Create the ring of a **named** namespace — the writer's half of
    /// [`GlyphRingReader::open_ns`]. `Err` on a namespace `$ORGANON_IPC_NS` would reject.
    pub fn create_ns(ns: &str, cell_aspect: f32, tick_hz: f32) -> io::Result<GlyphRingWriter> {
        let path = crate::ipc::glyph_ring_path_in(ns).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("'{ns}' is not a usable IPC namespace"),
            )
        })?;
        Self::create_at(&path, cell_aspect, tick_hz)
    }

    /// Create the ring at an explicit path (tests).
    pub fn create_at(path: &Path, cell_aspect: f32, tick_hz: f32) -> io::Result<GlyphRingWriter> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(GR_FILE_BYTES as u64)?;
        // SAFETY: the file is sized to GR_FILE_BYTES; this process is the sole writer.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let hdr = GlyphRingHeader { cell_aspect, tick_hz, ..Default::default() };
        map[..HDR_BYTES].copy_from_slice(bytemuck::bytes_of(&hdr));
        // Zero both slot headers so a reader never trusts a stale `seq` from a previous
        // run of a different size (the cells are not cleared — a slot is only ever read
        // after its header says which cells are live).
        for s in 0..GR_SLOTS {
            let o = slot_offset(s);
            map[o..o + FRAME_BYTES].copy_from_slice(bytemuck::bytes_of(&GlyphFrame::default()));
        }
        Ok(GlyphRingWriter { map, seq: 0, generation: 0, last: Vec::new(), last_dims: (0, 0) })
    }

    /// Frames published so far.
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Publish a grid. `meta.cols * meta.rows` must equal `cells.len()` and fit in
    /// `GR_MAX_CELLS`; `seq` and `generation` are stamped here (the caller's values are
    /// ignored), everything else in `meta` is written as given.
    ///
    /// Order is the whole protocol: fill the scratch slot (header then cells), THEN
    /// bump `write_seq`. A reader that observes the new `write_seq` observes a complete
    /// slot; one that observed the old value is reading the other slot.
    pub fn publish(&mut self, meta: &GlyphFrame, cells: &[GlyphCell]) -> io::Result<()> {
        let n = (meta.cols as usize) * (meta.rows as usize);
        if n != cells.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("grid is {}x{} = {} cells but {} were given", meta.cols, meta.rows, n, cells.len()),
            ));
        }
        if n > GR_MAX_CELLS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("grid of {n} cells exceeds the ring's {GR_MAX_CELLS}"),
            ));
        }
        let changed = self.last_dims != (meta.cols, meta.rows) || self.last.as_slice() != cells;
        if changed {
            self.generation = self.generation.wrapping_add(1);
            self.last.clear();
            self.last.extend_from_slice(cells);
            self.last_dims = (meta.cols, meta.rows);
        }
        let next = self.seq.wrapping_add(1).max(1);
        let slot = (next as usize - 1) % GR_SLOTS;
        let mut frame = *meta;
        frame.seq = next;
        frame.generation = self.generation;
        let o = slot_offset(slot);
        self.map[o..o + FRAME_BYTES].copy_from_slice(bytemuck::bytes_of(&frame));
        let c0 = o + FRAME_BYTES;
        self.map[c0..c0 + n * CELL_BYTES].copy_from_slice(bytemuck::cast_slice(cells));
        // Publish: `write_seq` is the 7th u32 of the header (offset 24).
        let seq_off = std::mem::offset_of!(GlyphRingHeader, write_seq);
        self.map[seq_off..seq_off + 4].copy_from_slice(bytemuck::bytes_of(&next));
        self.seq = next;
        Ok(())
    }
}

// ── Reader ──────────────────────────────────────────────────────────────────────

/// A grid as the reader hands it on: the frame header, the ring-level facts the
/// consumer needs beside it, and exactly `cols * rows` cells (top-down, row-major).
#[derive(Clone, Debug, Default)]
pub struct GlyphGrid {
    pub frame: GlyphFrame,
    pub cell_aspect: f32,
    pub tick_hz: f32,
    pub cells: Vec<GlyphCell>,
}

impl GlyphGrid {
    pub fn cols(&self) -> usize {
        self.frame.cols as usize
    }
    pub fn rows(&self) -> usize {
        self.frame.rows as usize
    }
    /// Cell at column `c`, row `r` — `r == 0` is the TOP row.
    pub fn at(&self, c: usize, r: usize) -> &GlyphCell {
        &self.cells[r * self.cols() + c]
    }
    pub fn settled(&self) -> bool {
        self.frame.flags & FRAME_SETTLED != 0
    }
}

/// Ring reader (the world). `latest` yields `None` until a writer has created the ring
/// and published a frame; the world re-opens lazily, as it does for the mind ring.
pub struct GlyphRingReader {
    map: Option<memmap2::Mmap>,
}

impl GlyphRingReader {
    /// Best-effort open in this process's namespace.
    pub fn open() -> GlyphRingReader {
        Self::open_at(&crate::ipc::glyph_ring_path())
    }

    /// Open the ring of a **named** namespace. `Err` for a name `$ORGANON_IPC_NS` would
    /// refuse (a typo, which never fixes itself); `Ok` with `is_open() == false` for a
    /// legal name whose producer has not started (which does).
    pub fn open_ns(ns: &str) -> Result<GlyphRingReader, String> {
        let path = crate::ipc::glyph_ring_path_in(ns).ok_or_else(|| {
            format!(
                "'{ns}' is not a usable IPC namespace — ASCII letters, digits, '-' and \
                 '_', 1..=64 characters (the same rule $ORGANON_IPC_NS obeys)"
            )
        })?;
        Ok(Self::open_at(&path))
    }

    /// Open at an explicit path (tests).
    pub fn open_at(path: &Path) -> GlyphRingReader {
        let map = OpenOptions::new().read(true).open(path).ok().and_then(|f| {
            if f.metadata().map(|m| m.len() as usize >= GR_FILE_BYTES).unwrap_or(false) {
                // SAFETY: the file is at least GR_FILE_BYTES.
                unsafe { memmap2::Mmap::map(&f).ok() }
            } else {
                None
            }
        });
        GlyphRingReader { map }
    }

    pub fn is_open(&self) -> bool {
        self.map.is_some()
    }

    fn header(&self) -> Option<GlyphRingHeader> {
        let m = self.map.as_ref()?;
        let h: GlyphRingHeader = bytemuck::pod_read_unaligned(&m[..HDR_BYTES]);
        if h.signature != GLYPH_RING_SIGNATURE
            || h.layout_version != GLYPH_LAYOUT_VERSION
            || h.cell_bytes as usize != CELL_BYTES
            || h.max_cells as usize != GR_MAX_CELLS
        {
            return None;
        }
        Some(h)
    }

    /// The published `write_seq`, or `None` while the ring is absent / refused. Cheap —
    /// a consumer polls this to learn whether [`latest_into`](Self::latest_into) would
    /// change anything.
    pub fn seq(&self) -> Option<u32> {
        self.header().map(|h| h.write_seq)
    }

    /// Copy the latest fully-published grid into `out`. `true` if `out` now holds a
    /// grid; `false` (and `out` untouched) if there is no ring, no frame yet, a layout
    /// disagreement, or the writer lapped the reader on every retry.
    pub fn latest_into(&self, out: &mut GlyphGrid) -> bool {
        let Some(m) = self.map.as_ref() else { return false };
        for _ in 0..3 {
            let Some(h) = self.header() else { return false };
            let seq0 = h.write_seq;
            if seq0 == 0 {
                return false;
            }
            let slot = (seq0 as usize - 1) % GR_SLOTS;
            let o = slot_offset(slot);
            let frame: GlyphFrame = bytemuck::pod_read_unaligned(&m[o..o + FRAME_BYTES]);
            if frame.seq != seq0 {
                continue; // lapped before we started; the header will say so next pass
            }
            let n = (frame.cols as usize) * (frame.rows as usize);
            if n > GR_MAX_CELLS {
                return false;
            }
            let c0 = o + FRAME_BYTES;
            // Per-cell unaligned reads rather than a `cast_slice`: the mmap is page-
            // aligned and the offsets are multiples of 4, so a cast would work today,
            // but a reader must not depend on an alignment the file format never
            // promised.
            let cells: Vec<GlyphCell> = (0..n)
                .map(|i| {
                    let a = c0 + i * CELL_BYTES;
                    bytemuck::pod_read_unaligned(&m[a..a + CELL_BYTES])
                })
                .collect();
            // Lap guard: an advance of one went to the OTHER slot and this copy is whole;
            // two or more means the writer came round to this slot mid-copy.
            let seq1 = bytemuck::pod_read_unaligned::<GlyphRingHeader>(&m[..HDR_BYTES]).write_seq;
            if seq1.wrapping_sub(seq0) >= GR_SLOTS as u32 {
                continue;
            }
            out.frame = frame;
            out.cell_aspect = h.cell_aspect;
            out.tick_hz = h.tick_hz;
            out.cells = cells;
            return true;
        }
        false
    }

    /// Convenience over [`latest_into`](Self::latest_into) that allocates.
    pub fn latest(&self) -> Option<GlyphGrid> {
        let mut g = GlyphGrid::default();
        self.latest_into(&mut g).then_some(g)
    }
}

// ── The blend clock (§7; T12's finding) ─────────────────────────────────────────
//
// The world draws a path character at `lerp(prev_exact, exact, blend)` (`lower_grid`),
// and `blend` is a clock: how far the frame being built sits between the two grids it
// holds. T12 read the world's version of that clock and found it wrong in a way no
// test had seen, because no test had a display: it measured time from the instant the
// world READ a grid, over a fixed `1 / tick_hz`, evaluated at BUILD time. At a producer
// faster than the display (the default: 120 Hz over 60 Hz) every frame reads a fresh
// grid at `since ≈ 0`, so `blend ≈ 0` on every frame and the tile sits at the grid read
// one frame EARLIER — `Exact`, one read late, two ticks behind, never between. At equal
// rates the same arithmetic draws one tick behind. A path only ever slid when the
// display outran the producer.
//
// Two things were wrong, and a publish stamp on the wire fixes only the smaller one.
// (1) The period was `1 / tick_hz` whatever the pair actually spanned: at 120/60 the
// two grids the world holds are two ticks apart. (2) The blend was evaluated at build
// time, for a frame that is SHOWN a display period later. Stamp the frames with the
// producer's clock and measure from the publish instead of the read, and 120/60 still
// lands at `blend ∈ 0..0.5` (the newest grid is 0–8 ms old at the read, over a 16.7 ms
// pair) — drawn between two and one ticks behind, never at the newest sample. What
// closes it is the LEAD: the world knows its own frame interval, and a frame built now
// is the picture at `now + one interval`. So
//
//     blend = (now − tick_at + lead) / period
//
// with `period` the producer-time span of the pair (`Δtick / tick_hz`, from what is
// already on the wire), `tick_at` the world instant the pair started and `lead` the
// world's last frame interval. 120/60: period 16.7 ms, lead 16.7 ms → 1 at every frame
// — the newest sample, two ticks a frame, uniform. 30/120: period 33 ms, lead 8.3 →
// 0.25, 0.5, 0.75, 1.0 — four interpolated frames. 60/60 → 1: the sample itself, not
// the one before it. A stalled producer clamps at 1 and holds. What `Slide` costs is
// now exactly one producer period, and only while the display outruns the producer.
//
// ⚠️ A heartbeat does not restart the clock. The settle publish and every dwell
// republish carry the SAME `tick` (`organon-glyphs/src/main.rs`): `classify_arrival`
// calls that a `Heartbeat`, the world replaces its current grid without rotating the
// previous one, and `tick_at` / `period` are untouched — so the last tick of an effect
// slides to completion under the settle frame instead of snapping to it, and T11's
// trails decaying through the dwell (a payload change at the same tick) move nothing.
// T5 and T11 depend on a heartbeat keeping the tile still; `heartbeat_does_not_restart_
// the_clock` pins it and names the mutation.
//
// ⚠️ Not a phase lock. The lead is the world's own frame interval, not an estimate of
// where the producer's clock is; at ratios that are neither `1:n` nor `n:1` (100 Hz over
// 60) the drawn step varies by up to a fraction of a tick per frame, because `Δtick`
// per read alternates and the lead does not. A locked render clock — a wall stamp on the
// frame, an offset estimated as a running minimum, a constant latency — would make that
// uniform too, at the cost of state that has to re-estimate on every epoch; it is the
// step to take if the GPU look shows judder at an odd ratio, and not before. All the
// arithmetic here is pure over synthetic seconds, so it is pinned without a display.

/// What a newly read frame is, relative to the grid the world is drawing — the answer
/// to "does this start a new slide, continue the current one, or replace the picture
/// without moving anything?".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrival {
    /// Nothing to slide from: the first frame ever, another effect run (`epoch`
    /// differs — a cut by definition), or a producer that restarted mid-run (`tick`
    /// went backwards). Drawn exactly, `blend = 1`, until the next tick.
    Cut,
    /// The next sample of the same run — `tick` advanced. Carries the pair's span in
    /// producer seconds, `(new.tick − cur.tick) / tick_hz`: one tick when the display
    /// keeps up, several when the producer outruns it.
    Tick(f32),
    /// A republish at the same `tick` and `epoch` — the settle publish, a dwell
    /// heartbeat, a T11 trail decaying under held text. The payload may differ (and
    /// `generation` may move) but no character moved: it replaces the current grid,
    /// the previous grid stays, and the blend clock runs on.
    Heartbeat,
}

/// Classify a frame the world has just read against the one it is drawing (`None`
/// before the first). `tick_hz` is the ring header's; a non-positive one falls back to
/// 60, as the world always has.
pub fn classify_arrival(cur: Option<&GlyphFrame>, new: &GlyphFrame, tick_hz: f32) -> Arrival {
    let Some(cur) = cur else { return Arrival::Cut };
    if new.epoch != cur.epoch {
        return Arrival::Cut;
    }
    match new.tick.cmp(&cur.tick) {
        std::cmp::Ordering::Equal => Arrival::Heartbeat,
        std::cmp::Ordering::Less => Arrival::Cut,
        std::cmp::Ordering::Greater => {
            let hz = if tick_hz > 0.0 { tick_hz } else { 60.0 };
            Arrival::Tick((new.tick - cur.tick) as f32 / hz)
        }
    }
}

/// The blend for a frame: `(since_tick + lead) / period`, clamped to `0..=1`, where
/// `since_tick` is how long ago (caller seconds) the current pair started, `lead` the
/// caller's own frame interval (a frame built now is shown one interval later) and
/// `period` the pair's producer-time span. `None` — no pair, a cut, a heartbeat that
/// never started one — is `1`: draw the current grid exactly. Never overshoots: a
/// stalled producer holds at the newest sample.
pub fn blend_for(since_tick: f32, lead: f32, period: Option<f32>) -> f32 {
    match period {
        Some(p) if p > 0.0 => ((since_tick.max(0.0) + lead.max(0.0)) / p).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// The world's blend clock over the ring: one per reader, fed every frame that is
/// read ([`arrive`](Self::arrive)) and asked once per frame built
/// ([`blend`](Self::blend)). Times are the caller's own seconds — any monotonic
/// origin, `f64` so an hours-long session keeps sub-millisecond resolution — which is
/// what lets the schedules in the tests run on synthetic time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendClock {
    /// When the current pair started (the last `Tick` or `Cut`).
    tick_at: f64,
    /// The pair's producer-time span; `None` until a `Tick` follows a `Cut`.
    period: Option<f32>,
    /// When the last frame was built — the next frame's lead is the interval since.
    last_build: Option<f64>,
}

impl BlendClock {
    /// Register a frame the world has just read, at caller time `now`, and say how to
    /// place it: `Heartbeat` replaces the current grid (the previous one stays, the
    /// clock is untouched); `Tick` and `Cut` rotate current → previous and restart the
    /// clock, with and without a period respectively.
    pub fn arrive(&mut self, cur: Option<&GlyphFrame>, new: &GlyphFrame, tick_hz: f32, now: f64) -> Arrival {
        let a = classify_arrival(cur, new, tick_hz);
        match a {
            Arrival::Heartbeat => {}
            Arrival::Tick(p) => {
                self.tick_at = now;
                self.period = Some(p);
            }
            Arrival::Cut => {
                self.tick_at = now;
                self.period = None;
            }
        }
        a
    }

    /// The blend for the frame being built at `now`. Records the build, so the next
    /// call's lead is this frame's interval; the first call has no lead (the frame is
    /// still one interval from being shown, but the interval is unknown, and 0 errs
    /// toward the earlier sample by less than a frame, once).
    pub fn blend(&mut self, now: f64) -> f32 {
        let lead = self.last_build.map_or(0.0, |b| (now - b).max(0.0) as f32);
        self.last_build = Some(now);
        let since = (now - self.tick_at).max(0.0) as f32;
        blend_for(since, lead, self.period)
    }

    /// The current pair's producer-time span, if a slide is in progress.
    pub fn period(&self) -> Option<f32> {
        self.period
    }
}

// ── Lowering: grid → instanced tiles (§3, §5, §7) ───────────────────────────────

/// The look constants T1 hard-codes. ⚠️ **T3 lifts every one of these onto the param
/// chain** (extrusion, bevel, face crown, emission gain, backplane material, camera
/// tilt); until then they are plain numbers here, in one place, named so the PR can
/// list them. Values are in **cell units** (§5.1): a column is `1.0` wide, a row is
/// `cell_aspect` tall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphLook {
    /// World units per column. Everything else scales with it.
    pub cell_w: f32,
    /// Full-block extrusion, in column widths. §5.1: at a near-orthographic camera the
    /// side walls are never seen, so this is a small number — a tile, not a pillar.
    pub depth: f32,
    /// Gap between the tile's back face and the backplane's front face, in column
    /// widths — the contact-shadow well.
    pub gap: f32,
    /// Emission gain, in SDR-white units (§4 — "denominate gain in SDR-white units,
    /// because the EDR path is real"). 1.0 = the terminal's brightness; above it the
    /// phosphor crosses the bloom threshold on its own.
    pub gain: f32,
    /// The faceplate: a near-black dielectric tint for every tile (§4's material
    /// sketch, `albedo 0.02–0.04`). Multiplies the mesh's RGB-cube colour (or replaces
    /// it under a palette — either way, dark).
    pub faceplate: [f32; 3],
    /// The backplane's tint, a shade lighter than the faceplate so the grid reads.
    pub backplane: [f32; 3],
    /// Backplane margin beyond the grid, in column widths.
    pub margin: f32,
    /// Backplane thickness, in column widths.
    pub backplane_depth: f32,
    /// Colour used for a cell that has a symbol but no foreground colour (ttfx's
    /// `colors: None` — the terminal's default foreground).
    pub default_fg: [f32; 3],
}

impl GlyphLook {
    /// The T1 look — the one set of numbers, `const` so the world can name it without
    /// a second copy that could drift. ⚠️ T3 lifts these onto the param chain.
    pub const DEFAULT: GlyphLook = GlyphLook {
        cell_w: 1.0,
        depth: 0.18,
        gap: 0.06,
        gain: 3.0,
        faceplate: [0.03, 0.03, 0.03],
        backplane: [0.06, 0.06, 0.065],
        margin: 1.5,
        backplane_depth: 0.25,
        default_fg: [0.75, 0.75, 0.75],
    };
}

impl Default for GlyphLook {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The instance triple the renderer draws: model matrices, tints and emissions, all
/// parallel. `lower_grid` appends to these; the caller owns clearing them.
pub struct TileOut<'a> {
    pub instances: &'a mut Vec<glam::Mat4>,
    pub tints: &'a mut Vec<glam::Vec4>,
    pub emits: &'a mut Vec<glam::Vec4>,
}

/// World-space centre of cell `(c, r)` — the grid is centred on the origin, `+x` right,
/// `+y` up, `+z` toward the camera. Row 0 is the top row (the ring's contract).
pub fn cell_centre(c: usize, r: usize, cols: usize, rows: usize, look: &GlyphLook, aspect: f32) -> glam::Vec3 {
    let w = look.cell_w;
    let h = look.cell_w * aspect;
    glam::Vec3::new(
        (c as f32 + 0.5 - cols as f32 * 0.5) * w,
        (rows as f32 * 0.5 - r as f32 - 0.5) * h,
        0.0,
    )
}

/// Lower a grid to instanced tiles: one rounded-box instance per non-empty cell, its
/// sub-cell extent and extrusion from [`tile_for`], its colour as **emission** (§4 —
/// decoded from sRGB first), a near-black faceplate as tint, plus one backplane slab
/// behind the grid. Returns the scene bounds so the camera can frame it.
///
/// **Motion (§7).** A tile sits at its cell's centre plus the cell's `sub_x`/`sub_y` —
/// the exact position the producer carried (zero from one that carries none). A cell
/// whose `SGR_ACTIVE_PATH` bit is set and whose `character_id` was in `prev` is drawn at
/// `lerp(prev_exact, exact, blend)`: the character is sliding along a path, so smoothing
/// between where it exactly was and where it exactly is changes where it is *between*
/// ticks and never when it arrives. A cell without the bit was placed by
/// `set_coordinate` — a cut — and is drawn where it is; interpolating it would invent
/// motion the effect never authored. `blend` is `0..=1`, the caller's [`BlendClock`]:
/// `(time since the pair started + the caller's frame interval) / the pair's
/// producer-time span`, so it reaches 1 exactly when the next tick is due and never
/// earlier than the frame it is built for is shown.
///
/// Extent, depth and position are all in cell units scaled by `look.cell_w`; the row
/// pitch honours `grid.cell_aspect` (§7: 2:1, or every ring becomes an ellipse).
///
/// This is [`lower_grid_with`] at [`LowerOptions::default()`] — T1's lowering exactly,
/// one tile per **non-empty** cell. The world calls this until the dark-tile lane is
/// wired.
pub fn lower_grid(grid: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32, look: &GlyphLook, out: TileOut<'_>) -> crate::math::Bounds {
    lower_grid_with(grid, prev, blend, look, LowerOptions::default(), out)
}

/// What [`lower_grid_with`] does beyond T1's lowering. Every field defaults to **off**,
/// and off is byte-identical to [`lower_grid`] (invariant #4, pinned by test) — so the
/// world can keep calling `lower_grid` and a preset saved before a switch existed lowers
/// the grid it lowered yesterday. A struct rather than a `bool` parameter so the next
/// lowering-only switch (T12's sub-cell rendering) is a field, not a signature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LowerOptions {
    /// **Every cell gets a tile** (T9, §15). A cell that draws no symbol — empty, space,
    /// a control — lowers as a full-cell tile at the shade-`░` depth (a quarter as proud
    /// as a lit glyph, on the shared `look.depth` scale), faceplate tint, and **zero**
    /// emission: the faceplate's environment sheen is all it shows, which is §4.1's dark
    /// cell that still shows the room. Symbol cells lower exactly as before; a T11
    /// trail is a *lit* cell with a small colour, never a dark one. Cost: `cols × rows`
    /// tiles per frame instead of one per glyph. Proposed wire: `Shared.glyph[14]`
    /// (`> 0.5`), `[13]` being the profile strength #233 named.
    pub dark_tiles: bool,
    /// **How a tile moves between the producer's ticks** (T12, §7). Defaults to
    /// [`Motion::Slide`], which is today's lowering exactly. Proposed wire:
    /// `Shared.glyph[15]` through [`Motion::from_lane`] (`[13]` is the profile strength,
    /// `[14]` the dark tiles; `[15]` is the last free slot of `glyph[16]`).
    pub motion: Motion,
}

/// How [`lower_grid_with`] places a tile between the producer's ticks (§7, T12). The
/// ring carries two signals: `SGR_ACTIVE_PATH` (the character is on a path, so its
/// position last tick and this tick are two samples of one motion) and `sub_x`/`sub_y`
/// (the exact sub-cell position the effect computed before rounding it to a cell). A
/// character *without* the bit was placed by `set_coordinate` — many effects teleport —
/// and its two positions are not samples of anything, so it **cuts** under every variant:
/// no variant ever interpolates a teleport, because that smears a scatter across the grid.
///
/// ⚠️ These are **not** a smoothing stack. `Slide` is one linear reconstruction between
/// two exact samples; `Exact` is the samples alone. Nothing here filters the remainder or
/// applies a second interpolation on top of the first, and once `blend` reaches 1 `Slide`
/// and `Exact` are byte-identical (pinned) — the world's blend only bridges the producer's
/// tick rate to the render rate, and a tick that arrives late clamps at 1 and holds. What
/// `Slide` costs is **up to one producer period of latency, and only while the display
/// outruns the producer**: the world can only interpolate toward the newest sample it
/// holds, so at 30 Hz over 120 the tile reaches each tick as the next one lands, and at
/// 120 over 60 (or 60 over 60) [`BlendClock`] puts it AT the newest sample on every frame
/// — the two-ticks-behind reading T12 took from the old clock is gone. `Exact` has none,
/// and at a 120 Hz producer the sub-cell path alone may be smooth enough — which is the
/// GPU question this switch exists to make askable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    /// Today's lowering (T1 + W6): a character on a path is drawn at
    /// `lerp(prev_exact, exact, blend)`, both ends being `centre + sub`; a teleport, a
    /// trail and a dark tile are drawn where they are.
    #[default]
    Slide,
    /// No inter-tick interpolation: every tile at its exact current position,
    /// `centre + sub`, whatever `blend` and `prev` say. The producer's sub-cell path is
    /// the only source of smoothness. Zero latency; steps at the producer's tick rate.
    Exact,
    /// The terminal's own picture, for the A/B §7 left open: every tile at its **cell
    /// centre**, the remainder ignored, nothing sliding. What a producer that carries no
    /// remainder shows under `Exact`; what a terminal shows always.
    Cells,
}

impl Motion {
    /// The proposed `Shared.glyph[15]` mapping: `0` = `Slide`, `1` = `Exact`, `2` =
    /// `Cells`, read to the nearest integer. **Anything else — including a NaN or a lane
    /// that was never written — is `Slide`**, so a snapshot that predates the lane and a
    /// garbage value both draw today's picture (invariant #4).
    pub fn from_lane(v: f32) -> Motion {
        match v.round() as i32 {
            1 => Motion::Exact,
            2 => Motion::Cells,
            _ => Motion::Slide,
        }
    }
}

/// The tile a symbol-less cell becomes under [`LowerOptions::dark_tiles`]: the whole
/// cell, at the `░` depth, emitting nothing. `emission` is what makes it dark — the
/// lowering multiplies the cell's colour by it, so a dark tile's emit is exactly zero
/// whatever colour a producer left in an empty cell's `fg`.
pub const DARK_TILE: Tile = Tile { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0, depth: 0.25, emission: 0.0 };

/// [`lower_grid`] with [`LowerOptions`]. See both.
pub fn lower_grid_with(grid: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32, look: &GlyphLook, opts: LowerOptions, out: TileOut<'_>) -> crate::math::Bounds {
    use glam::{Mat4, Quat, Vec3, Vec4};
    let (cols, rows) = (grid.cols(), grid.rows());
    let aspect = if grid.cell_aspect > 0.0 { grid.cell_aspect } else { TTFX_CELL_ASPECT };
    let w = look.cell_w;
    let h = w * aspect;
    let blend = blend.clamp(0.0, 1.0);
    let mut bounds = crate::math::Bounds::new();
    if cols == 0 || rows == 0 {
        return bounds;
    }
    // Where a character exactly is: its cell's centre plus the sub-cell remainder the
    // producer carried (§7; zero from a producer that carries none). Under
    // `Motion::Cells` the remainder is ignored — the terminal's own picture (T12).
    let use_sub = opts.motion != Motion::Cells;
    let exact = |c: usize, r: usize, cell: &GlyphCell| {
        let mut p = cell_centre(c, r, cols, rows, look, aspect);
        if use_sub {
            p.x += cell.sub_x * w;
            p.y += cell.sub_y * h;
        }
        p
    };
    // Where each character exactly was last frame, for the slide. Built only under
    // `Motion::Slide`, when a previous grid exists and the blend is not already
    // complete (then the answer is `exact` of the current cell). An empty map is what
    // makes `Exact` and `Cells` never interpolate: nothing has an origin.
    let prev_pos: std::collections::HashMap<u32, Vec3> = match prev {
        Some(p) if opts.motion == Motion::Slide && blend < 1.0 && p.cols() == cols && p.rows() == rows => {
            let mut m = std::collections::HashMap::with_capacity(p.cells.len() / 4 + 1);
            for (i, cell) in p.cells.iter().enumerate() {
                // A trail (T11) keeps the `character_id` of the character that left it,
                // and that character is usually ALSO live somewhere else in the same
                // grid. A trail is where the character WAS two or more ticks ago, never
                // where it was last tick — so it is never a slide's origin. Without this
                // the later index would win the map and a sliding character would start
                // each tick from its own trail.
                if cell.symbol != 0 && cell.sgr & SGR_PERSIST == 0 && tile_for(cell.symbol).is_some() {
                    m.insert(cell.character_id, exact(i % cols, i / cols, cell));
                }
            }
            m
        }
        _ => std::collections::HashMap::new(),
    };
    let faceplate = Vec4::new(look.faceplate[0], look.faceplate[1], look.faceplate[2], 1.0);
    for r in 0..rows {
        for c in 0..cols {
            let cell = grid.at(c, r);
            let (tile, centre, dark) = match tile_for(cell.symbol) {
                Some(tile) => {
                    // The slide runs between the two EXACT positions — never between
                    // cell centres with this tick's remainder added on top, which would
                    // start every tick with a jump back toward the cell boundary.
                    // `lerp(a, a, t)` is `a`, so a character that did not move is drawn
                    // where it is.
                    let mut centre = exact(c, r, cell);
                    if cell.sgr & SGR_ACTIVE_PATH != 0 {
                        if let Some(&from) = prev_pos.get(&cell.character_id) {
                            centre = from.lerp(centre, blend);
                        }
                    }
                    (tile, centre, false)
                }
                // A dark cell is the faceplate's cell, not a character's: it sits on the
                // grid at the cell's centre and never slides, whatever `sub_x`/`sub_y`
                // or the path bit say (a space character on a path is a real thing in
                // ttfx, and its tile must not go for a walk).
                None if opts.dark_tiles => (DARK_TILE, cell_centre(c, r, cols, rows, look, aspect), true),
                None => continue,
            };
            let sx = (tile.x1 - tile.x0) * w;
            let sy = (tile.y1 - tile.y0) * h;
            let sz = (look.depth * tile.depth * w).max(1e-4);
            let ox = ((tile.x0 + tile.x1) * 0.5 - 0.5) * w;
            let oy = ((tile.y0 + tile.y1) * 0.5 - 0.5) * h;
            let pos = Vec3::new(centre.x + ox, centre.y + oy, sz * 0.5);
            out.instances.push(Mat4::from_scale_rotation_translation(Vec3::new(sx, sy, sz), Quat::IDENTITY, pos));
            out.tints.push(faceplate);
            let rgb = if cell.sgr & SGR_HAS_FG != 0 { linear_rgb(cell.fg) } else { look.default_fg };
            let e = tile.emission;
            out.emits.push(Vec4::new(rgb[0] * e, rgb[1] * e, rgb[2] * e, look.gain));
            // The bounds frame the camera and are unchanged by dark tiles: a dark tile
            // is inside the backplane's footprint and a quarter as proud as any lit
            // glyph, so the switch cannot move the camera.
            if !dark {
                bounds.min = bounds.min.min(pos - Vec3::new(sx, sy, sz) * 0.5);
                bounds.max = bounds.max.max(pos + Vec3::new(sx, sy, sz) * 0.5);
            }
        }
    }
    // The backplane: one slab behind the whole grid, its front face `gap` behind the
    // tiles' back faces (which sit at z = 0). Real geometry, so hardware RT sees it
    // (§5) and the tiles' contact shadows land on it.
    let bw = cols as f32 * w + 2.0 * look.margin * w;
    let bh = rows as f32 * h + 2.0 * look.margin * w;
    let bd = (look.backplane_depth * w).max(1e-4);
    let bz = -(look.gap * w) - bd * 0.5;
    out.instances.push(Mat4::from_scale_rotation_translation(Vec3::new(bw, bh, bd), Quat::IDENTITY, Vec3::new(0.0, 0.0, bz)));
    out.tints.push(Vec4::new(look.backplane[0], look.backplane[1], look.backplane[2], 1.0));
    out.emits.push(Vec4::ZERO);
    bounds.min = bounds.min.min(Vec3::new(-bw * 0.5, -bh * 0.5, bz - bd * 0.5));
    bounds.max = bounds.max.max(Vec3::new(bw * 0.5, bh * 0.5, bz + bd * 0.5));
    bounds
}

#[cfg(test)]
mod lower_tests {
    use super::*;
    use glam::{Mat4, Vec4};

    fn grid_of(cols: u32, rows: u32, cells: Vec<GlyphCell>) -> GlyphGrid {
        GlyphGrid {
            frame: GlyphFrame { cols, rows, ..Default::default() },
            cell_aspect: 2.0,
            tick_hz: 60.0,
            cells,
        }
    }

    fn c(sym: char, id: u32, sgr: u32) -> GlyphCell {
        GlyphCell { symbol: sym as u32, character_id: id, sgr: sgr | SGR_HAS_FG, fg: pack_rgb(255, 128, 0), ..Default::default() }
    }

    fn lower(g: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32) -> (Vec<Mat4>, Vec<Vec4>, Vec<Vec4>, crate::math::Bounds) {
        let (mut i, mut t, mut e) = (Vec::new(), Vec::new(), Vec::new());
        let b = lower_grid(g, prev, blend, &GlyphLook::default(), TileOut { instances: &mut i, tints: &mut t, emits: &mut e });
        (i, t, e, b)
    }

    /// One tile per non-empty cell plus the backplane; the three vectors stay parallel
    /// (a length mismatch is what the renderer treats as "no emission").
    #[test]
    fn one_instance_per_glyph_plus_one_backplane_and_the_buffers_are_parallel() {
        let g = grid_of(3, 1, vec![c('█', 1, 0), GlyphCell::default(), c('▄', 2, 0)]);
        let (i, t, e, b) = lower(&g, None, 1.0);
        assert_eq!(i.len(), 3, "two glyphs + the backplane");
        assert_eq!(t.len(), i.len());
        assert_eq!(e.len(), i.len());
        assert_eq!(e[2], Vec4::ZERO, "the backplane does not emit");
        assert!(b.min.is_finite() && b.max.is_finite());
        assert!(b.max.x > b.min.x && b.max.y > b.min.y);
    }

    /// The 2:1 cell: a full block is twice as tall as it is wide, and the lower half
    /// block sits in the BOTTOM half of its cell.
    #[test]
    fn the_cell_aspect_and_the_sub_cell_extent_are_honoured() {
        let g = grid_of(1, 1, vec![c('█', 1, 0)]);
        let (i, ..) = lower(&g, None, 1.0);
        let (s, _, _) = i[0].to_scale_rotation_translation();
        assert!((s.y / s.x - 2.0).abs() < 1e-5, "{s}");
        let g = grid_of(1, 1, vec![c('▄', 1, 0)]);
        let (i, ..) = lower(&g, None, 1.0);
        let (s, _, p) = i[0].to_scale_rotation_translation();
        assert!((s.y - 1.0).abs() < 1e-5, "half of a 2-tall cell");
        assert!(p.y < 0.0, "lower half sits below the cell centre: {p}");
        // Shade → depth, on the shared depth scale.
        let full = lower(&grid_of(1, 1, vec![c('█', 1, 0)]), None, 1.0).0[0].to_scale_rotation_translation().0.z;
        let light = lower(&grid_of(1, 1, vec![c('░', 1, 0)]), None, 1.0).0[0].to_scale_rotation_translation().0.z;
        assert!((light / full - 0.25).abs() < 1e-5);
    }

    /// Emission is the DECODED colour times gain; the tint is the faceplate, never the
    /// colour (§4 — colour is emission, not albedo).
    #[test]
    fn colour_goes_to_emission_decoded_and_the_tint_is_the_faceplate() {
        let g = grid_of(1, 1, vec![c('█', 1, 0)]);
        let (_, t, e, _) = lower(&g, None, 1.0);
        let look = GlyphLook::default();
        assert_eq!(t[0], Vec4::new(look.faceplate[0], look.faceplate[1], look.faceplate[2], 1.0));
        assert!((e[0].x - 1.0).abs() < 1e-6, "255 → 1.0");
        assert!((e[0].y - srgb8_to_linear(128)).abs() < 1e-6, "128 decoded, not 0.502");
        assert_eq!(e[0].z, 0.0);
        assert_eq!(e[0].w, look.gain);
        // The unknown-symbol rule reaches the buffer: a letter emits less than a block.
        let a = lower(&grid_of(1, 1, vec![c('A', 1, 0)]), None, 1.0).2[0];
        assert!(a.x < e[0].x && a.x > 0.0);
    }

    /// §7: a character on an active path slides from where it was; one that was
    /// teleported (no bit) does not; at blend 1 both are at their new cell.
    #[test]
    fn active_path_slides_and_a_cut_does_not() {
        // Frame A: id 7 at (0,0). Frame B: id 7 at (2,0), once with the path bit, once without.
        let a = grid_of(3, 1, vec![c('█', 7, 0), GlyphCell::default(), GlyphCell::default()]);
        let slide = grid_of(3, 1, vec![GlyphCell::default(), GlyphCell::default(), c('█', 7, SGR_ACTIVE_PATH)]);
        let cut = grid_of(3, 1, vec![GlyphCell::default(), GlyphCell::default(), c('█', 7, 0)]);
        let x_at = |g: &GlyphGrid, prev, blend| lower(g, prev, blend).0[0].to_scale_rotation_translation().2.x;
        let x0 = x_at(&a, None, 1.0);
        let x2 = x_at(&slide, None, 1.0);
        assert!(x2 > x0);
        let mid = x_at(&slide, Some(&a), 0.5);
        assert!((mid - (x0 + x2) * 0.5).abs() < 1e-5, "half-way at blend 0.5: {mid}");
        assert_eq!(x_at(&slide, Some(&a), 0.0), x0, "blend 0 = still at the old cell");
        assert_eq!(x_at(&slide, Some(&a), 1.0), x2, "blend 1 = arrived");
        assert_eq!(x_at(&cut, Some(&a), 0.5), x2, "a cut is drawn where it is, never between");
        // A previous grid of another size cannot be matched cell-for-cell: no slide.
        let other = grid_of(2, 1, vec![c('█', 7, 0), GlyphCell::default()]);
        assert_eq!(x_at(&slide, Some(&other), 0.5), x2);
    }

    /// §7, retired: the producer carries the remainder the effect's rounding dropped and
    /// the tile sits there — `+x` right, `+y` up, in cells. And a slide runs between the
    /// two EXACT positions: a character at 0.3 cells/tick must never jump back toward a
    /// cell boundary at the start of a tick, which is what lerping cell centres and then
    /// adding the new tick's remainder would do.
    #[test]
    fn the_sub_cell_offset_places_the_tile_and_the_slide_runs_between_exact_positions() {
        let with = |sx: f32, sy: f32, sgr: u32| GlyphCell { sub_x: sx, sub_y: sy, ..c('█', 7, sgr) };
        let at = |g: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32| lower(g, prev, blend).0[0].to_scale_rotation_translation().2;
        let look = GlyphLook::default();
        let (w, h) = (look.cell_w, look.cell_w * 2.0);
        let e = GlyphCell::default;
        let zero = grid_of(3, 1, vec![e(), c('█', 7, 0), e()]);
        let off = grid_of(3, 1, vec![e(), with(0.25, -0.5, 0), e()]);
        let p0 = at(&zero, None, 1.0);
        let p1 = at(&off, None, 1.0);
        assert!((p1.x - p0.x - 0.25 * w).abs() < 1e-5, "sub_x moves the tile RIGHT by a quarter cell: {p0} -> {p1}");
        assert!((p1.y - p0.y + 0.5 * h).abs() < 1e-5, "sub_y = -0.5 moves it DOWN by half a (2:1) cell: {p0} -> {p1}");
        // Last tick: cell 0 with remainder +0.4 (x = 0.4 cells). This tick: cell 1 with
        // remainder -0.3 (x = 0.7 cells). Blend 0 is 0.4 — not cell 0's centre, and not
        // cell 0's centre minus 0.3 — and blend 1 is 0.7.
        let prev = grid_of(3, 1, vec![with(0.4, 0.0, SGR_ACTIVE_PATH), e(), e()]);
        let cur = grid_of(3, 1, vec![e(), with(-0.3, 0.0, SGR_ACTIVE_PATH), e()]);
        let x_cell0 = at(&grid_of(3, 1, vec![c('█', 7, 0), e(), e()]), None, 1.0).x;
        let start = at(&cur, Some(&prev), 0.0).x;
        let end = at(&cur, Some(&prev), 1.0).x;
        assert!((start - (x_cell0 + 0.4 * w)).abs() < 1e-5, "blend 0 starts where the character exactly WAS: {start}");
        assert!((end - (x_cell0 + 0.7 * w)).abs() < 1e-5, "blend 1 lands where it exactly IS: {end}");
        assert!(start < end, "the slide runs forward, never back toward the boundary");
        let mid = at(&cur, Some(&prev), 0.5).x;
        assert!((mid - (start + end) * 0.5).abs() < 1e-5);
        // A cut with a remainder is drawn at its exact position, never between.
        let cut = grid_of(3, 1, vec![e(), with(-0.3, 0.0, 0), e()]);
        assert_eq!(at(&cut, Some(&prev), 0.5).x, end);
    }

    #[test]
    fn an_empty_grid_lowers_to_nothing() {
        let g = grid_of(0, 0, vec![]);
        let (i, ..) = lower(&g, None, 1.0);
        assert!(i.is_empty());
    }

    /// T11: a trail keeps the id of the character that left it, and that character is
    /// live elsewhere in the same grid. The slide must start from where the character
    /// WAS — its live cell last tick — never from its trail, which is where it was two
    /// ticks ago. The trail sits at the HIGHER index so that, without the exclusion, it
    /// is the one the map keeps.
    #[test]
    fn a_trail_is_never_a_slide_origin() {
        let e = GlyphCell::default;
        // Last tick: id 7 live at cell 0, and its trail (from the tick before) at cell 2.
        let prev = grid_of(3, 1, vec![c('█', 7, SGR_ACTIVE_PATH), e(), c('█', 7, SGR_PERSIST)]);
        // This tick: id 7 live at cell 1.
        let cur = grid_of(3, 1, vec![e(), c('█', 7, SGR_ACTIVE_PATH), e()]);
        let x_of = |g: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32, n: usize| lower(g, prev, blend).0[n].to_scale_rotation_translation().2.x;
        let x_cell0 = x_of(&grid_of(3, 1, vec![c('█', 7, 0), e(), e()]), None, 1.0, 0);
        let x_cell2 = x_of(&grid_of(3, 1, vec![e(), e(), c('█', 7, 0)]), None, 1.0, 0);
        let start = x_of(&cur, Some(&prev), 0.0, 0);
        assert!((start - x_cell0).abs() < 1e-5 && (start - x_cell2).abs() > 0.5, "the slide starts at the LIVE cell (x={x_cell0}), not the trail (x={x_cell2}): {start}");
        // And a trail in the current grid is drawn where it is, whatever the previous
        // grid says about its id: it carries no path bit, so it cannot slide.
        let trail_now = grid_of(3, 1, vec![e(), c('█', 7, SGR_ACTIVE_PATH), c('█', 7, SGR_PERSIST)]);
        assert_eq!(x_of(&trail_now, Some(&prev), 0.5, 1), x_cell2, "a trail never moves");
        // The trail still lowers to a tile, at its (decayed) colour — the renderer does
        // not need to know the bit to draw the right picture.
        assert_eq!(lower(&trail_now, None, 1.0).0.len(), 3, "live + trail + backplane");
    }

    // ── T9: every cell gets a tile ──────────────────────────────────────────────

    const DARK: LowerOptions = LowerOptions { dark_tiles: true, motion: Motion::Slide };

    fn lower_opts(g: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32, opts: LowerOptions) -> (Vec<Mat4>, Vec<Vec4>, Vec<Vec4>, crate::math::Bounds) {
        let (mut i, mut t, mut e) = (Vec::new(), Vec::new(), Vec::new());
        let b = lower_grid_with(g, prev, blend, &GlyphLook::default(), opts, TileOut { instances: &mut i, tints: &mut t, emits: &mut e });
        (i, t, e, b)
    }

    /// The asymmetric fixture every T9 test lowers: 4 wide, 2 tall, holes in different
    /// places on each row, a sliding character with a remainder, a letter (the
    /// unknown-symbol rule), a trail, and a space CHARACTER on a path — a real ttfx
    /// thing, and the one cell that must be dark yet must not move.
    fn fixture() -> (GlyphGrid, GlyphGrid) {
        let e = GlyphCell::default;
        let sp = |id: u32| GlyphCell { symbol: 0x20, character_id: id, sgr: SGR_ACTIVE_PATH | SGR_HAS_FG, fg: pack_rgb(255, 255, 255), sub_x: 0.4, sub_y: 0.2, ..Default::default() };
        let slide = |id: u32, sx: f32| GlyphCell { sub_x: sx, ..c('▀', id, SGR_ACTIVE_PATH) };
        let trail = GlyphCell { fg: pack_rgb(9, 40, 3), ..c('█', 7, SGR_PERSIST) };
        let prev = grid_of(4, 2, vec![slide(7, 0.3), e(), c('A', 8, 0), sp(9), e(), e(), e(), e()]);
        let cur = grid_of(4, 2, vec![e(), slide(7, -0.2), c('A', 8, 0), e(), trail, e(), e(), sp(9)]);
        (prev, cur)
    }

    /// §15's pin: with the switch on, `cols × rows + 1` instances, the buffers parallel.
    #[test]
    fn with_dark_tiles_on_every_cell_gets_a_tile_plus_the_backplane() {
        let (prev, cur) = fixture();
        let (i, t, e, _) = lower_opts(&cur, Some(&prev), 0.5, DARK);
        assert_eq!(i.len(), 4 * 2 + 1, "cols × rows + 1");
        assert_eq!(t.len(), i.len());
        assert_eq!(e.len(), i.len());
        assert_eq!(e[i.len() - 1], Vec4::ZERO, "the backplane is still last and still does not emit");
        // The logo's number from §15's spec: an 81×10 grid is 810 tiles + 1.
        let logo = grid_of(81, 10, vec![GlyphCell::default(); 810]);
        assert_eq!(lower_opts(&logo, None, 1.0, DARK).0.len(), 811);
        assert_eq!(lower(&logo, None, 1.0).0.len(), 1, "…and off, an empty grid is the backplane alone");
    }

    /// A dark cell's emit is EXACTLY zero (not small — zero, so the bloom prefilter and
    /// the brightest-N light selection can never pick one), its tint is the faceplate,
    /// its depth is a quarter of `look.depth` — the `░` depth, on the shared scale —
    /// and it fills its cell.
    #[test]
    fn a_dark_tile_emits_exactly_nothing_and_is_a_quarter_as_proud_as_a_lit_glyph() {
        let look = GlyphLook::default();
        let g = grid_of(2, 1, vec![GlyphCell::default(), c('█', 1, 0)]);
        let (i, t, e, _) = lower_opts(&g, None, 1.0, DARK);
        assert_eq!(e[0].truncate(), glam::Vec3::ZERO, "a dark cell emits nothing: {}", e[0]);
        assert_eq!(e[0].w, look.gain, "the gain lane is kept so the buffer stays uniform");
        assert_eq!(t[0], Vec4::new(look.faceplate[0], look.faceplate[1], look.faceplate[2], 1.0));
        // Read the matrix directly — a scale/rotation/translation decomposition goes
        // through a square root, and "exactly" means exactly.
        let (s, p) = (glam::Vec3::new(i[0].x_axis.x, i[0].y_axis.y, i[0].z_axis.z), i[0].w_axis.truncate());
        assert_eq!(s.z, look.depth * 0.25 * look.cell_w, "exactly 0.25 × look.depth");
        let full = glam::Vec3::new(i[1].x_axis.x, i[1].y_axis.y, i[1].z_axis.z);
        assert!((full.z / s.z - 4.0).abs() < 1e-5, "a lit full block is four times as proud: {} vs {}", full.z, s.z);
        assert_eq!((s.x, s.y), (look.cell_w, look.cell_w * 2.0), "the whole cell, at the 2:1 aspect");
        assert_eq!(p.z, s.z * 0.5, "its back face sits on z = 0 like every tile");
        assert_eq!(p.truncate(), cell_centre(0, 0, 2, 1, &look, 2.0).truncate(), "at its cell centre");
        // An empty cell's colour is whatever the producer left there; it is never emitted.
        let stale = GlyphCell { fg: pack_rgb(255, 255, 255), sgr: SGR_HAS_FG, ..Default::default() };
        let (_, _, e, _) = lower_opts(&grid_of(1, 1, vec![stale]), None, 1.0, DARK);
        assert_eq!(e[0].truncate(), glam::Vec3::ZERO, "a stale fg in an empty cell must not light it");
        // And the dark depth IS the shade block's depth — one number, not two.
        assert_eq!(DARK_TILE.depth, tile_for('░' as u32).unwrap().depth);
    }

    /// A T11 trail is a lit cell with a small colour, never a dark one: the rule fires
    /// only on symbol-less cells. Its depth is its symbol's and its emit its decayed
    /// colour.
    #[test]
    fn a_persist_trail_still_lowers_as_a_lit_cell() {
        let trail = GlyphCell { fg: pack_rgb(9, 40, 3), ..c('█', 7, SGR_PERSIST) };
        let (i, _, e, _) = lower_opts(&grid_of(1, 1, vec![trail]), None, 1.0, DARK);
        let want = linear_rgb(pack_rgb(9, 40, 3));
        assert_eq!(e[0].truncate(), glam::Vec3::from(want), "a trail emits its decayed colour, dim but not zero");
        assert!(e[0].y > 0.0);
        let look = GlyphLook::default();
        assert_eq!(i[0].z_axis.z, look.depth * look.cell_w, "a full-block trail is a full-depth tile");
    }

    /// A dark cell never slides: a space CHARACTER on a path (a real ttfx thing) with a
    /// remainder and a previous position is a dark tile at its cell centre — the tile
    /// is the faceplate's, not the character's.
    #[test]
    fn a_dark_tile_sits_at_its_cell_centre_and_never_slides() {
        let (prev, cur) = fixture();
        let look = GlyphLook::default();
        // The space character (id 9) is at cell (3, 1) in `cur`, (3, 0) in `prev`.
        let (i, ..) = lower_opts(&cur, Some(&prev), 0.5, DARK);
        let p = i[7].w_axis.truncate();
        assert_eq!(p.truncate(), cell_centre(3, 1, 4, 2, &look, 2.0).truncate(), "not slid, not offset: {p}");
        // While the real sliding character (id 7, cell 1 of row 0) still slides.
        let x0 = i[1].to_scale_rotation_translation().2.x;
        let x1 = lower_opts(&cur, Some(&prev), 1.0, DARK).0[1].to_scale_rotation_translation().2.x;
        assert!(x0 < x1, "the block on a path is mid-slide at blend 0.5: {x0} < {x1}");
    }

    /// Invariant #4: off is byte-identical to T1's lowering — every instance, tint and
    /// emit, in order, and the bounds — and `lower_grid` IS the default-options call.
    /// And on, the bounds do not move: a dark tile cannot re-frame the camera.
    #[test]
    fn dark_tiles_off_is_byte_identical_to_lower_grid_and_on_leaves_the_bounds_alone() {
        let (prev, cur) = fixture();
        for (blend, p) in [(0.5, Some(&prev)), (1.0, None), (0.0, Some(&prev))] {
            let (i0, t0, e0, b0) = lower(&cur, p, blend);
            let (i1, t1, e1, b1) = lower_opts(&cur, p, blend, LowerOptions::default());
            assert_eq!(i0, i1, "instances differ with the switch off (blend {blend})");
            assert_eq!(t0, t1);
            assert_eq!(e0, e1);
            assert_eq!((b0.min, b0.max), (b1.min, b1.max));
            let (i2, _, e2, b2) = lower_opts(&cur, p, blend, DARK);
            assert_eq!((b0.min, b0.max), (b2.min, b2.max), "the bounds are unchanged by dark tiles (blend {blend})");
            assert_eq!(i2.len(), i0.len() + 5, "the fixture has five symbol-less cells (four empty, one space character): {} -> {}", i0.len(), i2.len());
            // Every lit tile is where it was, in order, with the dark ones interleaved.
            let lit: Vec<&Mat4> = i2.iter().zip(&e2).filter(|(_, e)| e.truncate() != glam::Vec3::ZERO).map(|(m, _)| m).collect();
            assert_eq!(lit, i0[..i0.len() - 1].iter().collect::<Vec<_>>(), "lit tiles are byte-identical with the switch on");
        }
        assert_eq!(LowerOptions::default(), LowerOptions { dark_tiles: false, motion: Motion::Slide }, "every option defaults to off / today");
        // The case a lit fixture cannot see: with NO lit cell, folding dark tiles into
        // the bounds would lift `max.z` off the backplane's face to a quarter depth
        // and the camera would frame a different box for the same grid.
        let empty = grid_of(5, 3, vec![GlyphCell::default(); 15]);
        let (_, _, _, b_off) = lower(&empty, None, 1.0);
        let (_, _, _, b_on) = lower_opts(&empty, None, 1.0, DARK);
        assert_eq!((b_off.min, b_off.max), (b_on.min, b_on.max), "an all-dark grid frames exactly as an empty one: {:?} vs {:?}", b_off.max, b_on.max);
        assert!(b_on.max.z < 0.0, "…which is the backplane alone, wholly behind z = 0: {}", b_on.max.z);
    }

    /// §15.2's "not yet measured": the ~16 000-cell fullscreen case. Prints, never
    /// gates — run `cargo test -p organon-core --release fullscreen -- --nocapture` for
    /// a number that means something (an unoptimised build measures a different program).
    #[test]
    fn fullscreen_lowering_cost_is_printed_not_gated() {
        use std::time::Instant;
        let (cols, rows) = (200usize, 80usize);
        // A plausible fullscreen frame: ~one cell in seven lit, on paths, with remainders.
        let mk = |shift: usize| {
            let cells = (0..cols * rows)
                .map(|i| if (i + shift) % 7 == 0 { GlyphCell { sub_x: 0.1, ..c('█', i as u32, SGR_ACTIVE_PATH) } } else { GlyphCell::default() })
                .collect();
            grid_of(cols as u32, rows as u32, cells)
        };
        let (prev, cur) = (mk(0), mk(1));
        let look = GlyphLook::default();
        // Three conditions, measured in interleaved rounds (best of all runs per
        // condition) so that clock ramp, cache warmth and allocator state do not land
        // on whichever condition happened to run first — the first draft ran them in
        // a fixed order and "on" came out faster than "off".
        let conds: [(&str, LowerOptions, Option<&GlyphGrid>); 3] =
            [("off, sliding", LowerOptions::default(), Some(&prev)), ("on, sliding", DARK, Some(&prev)), ("on, settled", DARK, None)];
        let mut best = [f64::MAX; 3];
        let mut count = [0usize; 3];
        let (mut i, mut t, mut e) = (Vec::with_capacity(cols * rows + 1), Vec::with_capacity(cols * rows + 1), Vec::with_capacity(cols * rows + 1));
        for _round in 0..5 {
            for (k, (_, opts, p)) in conds.iter().enumerate() {
                for _ in 0..10 {
                    i.clear();
                    t.clear();
                    e.clear();
                    let t0 = Instant::now();
                    lower_grid_with(&cur, *p, 0.5, &look, *opts, TileOut { instances: &mut i, tints: &mut t, emits: &mut e });
                    best[k] = best[k].min(t0.elapsed().as_secs_f64() * 1e6);
                    count[k] = i.len();
                }
            }
        }
        let (n_off, us_off) = (count[0], best[0]);
        let (n_on, us_on, us_on_still) = (count[1], best[1], best[2]);
        assert_eq!(n_on, cols * rows + 1);
        println!(
            "fullscreen {cols}x{rows} = {} cells: off {n_off} instances, best {us_off:.0} us; \
             dark tiles on {n_on} instances, best {us_on:.0} us (sliding), {us_on_still:.0} us (settled) \
             [{} build]",
            cols * rows,
            if cfg!(debug_assertions) { "DEBUG — not the number, rerun --release" } else { "release" }
        );
    }

    // ── T12: slide on a path, cut on a teleport ─────────────────────────────────

    const EXACT: LowerOptions = LowerOptions { dark_tiles: false, motion: Motion::Exact };
    const CELLS: LowerOptions = LowerOptions { dark_tiles: false, motion: Motion::Cells };

    fn xy(i: &[Mat4], n: usize) -> (f32, f32) {
        let p = i[n].to_scale_rotation_translation().2;
        (p.x, p.y)
    }

    /// §7's rule on one two-tick grid, under the default motion: a path character with a
    /// remainder is at `centre + sub` once the tick completes and half-way between its
    /// two exact positions at blend 0.5; a teleport (no `ACTIVE_PATH`, cell changed) is
    /// at its NEW cell at every blend — a cut, never a smear; a trail is never a slide's
    /// origin even when it sits at the higher index, and never moves itself.
    #[test]
    fn t12_a_two_tick_grid_slides_the_path_cuts_the_teleport_and_never_starts_from_a_trail() {
        let e = GlyphCell::default;
        let look = GlyphLook::default();
        let (cols, rows) = (4usize, 2usize);
        let w = look.cell_w;
        let centre = |c: usize, r: usize| cell_centre(c, r, cols, rows, &look, 2.0);
        // Last tick: id 7 live on a path at (0,0) with remainder +0.4, its trail (two
        // ticks old) at (3,0) — the HIGHER index, so it would win the origin map without
        // T11's exclusion — and id 8 sitting at (0,1) with no path bit.
        let prev = grid_of(4, 2, vec![
            GlyphCell { sub_x: 0.4, ..c('█', 7, SGR_ACTIVE_PATH) }, e(), e(), c('█', 7, SGR_PERSIST),
            c('█', 8, 0), e(), e(), e(),
        ]);
        // This tick: id 7 at (1,0) with remainder −0.3, its trail where it just was, and
        // id 8 teleported by `set_coordinate` to (3,1).
        let cur = grid_of(4, 2, vec![
            GlyphCell { sub_x: 0.4, ..c('█', 7, SGR_PERSIST) }, GlyphCell { sub_x: -0.3, ..c('█', 7, SGR_ACTIVE_PATH) }, e(), e(),
            e(), e(), e(), c('█', 8, 0),
        ]);
        // Instance order: the trail (0), the path character (1), the teleport (2), the backplane.
        let at = |blend: f32, p: Option<&GlyphGrid>| lower_opts(&cur, p, blend, LowerOptions::default()).0;
        let was = centre(0, 0).x + 0.4 * w;
        let is = centre(1, 0).x - 0.3 * w;

        // Blend 1: exactly `centre + sub`, and no trace of `prev` — with it, without it,
        // and past 1 (a late tick clamps and holds).
        for (label, i) in [("blend 1 with prev", at(1.0, Some(&prev))), ("blend 1 no prev", at(1.0, None)), ("blend 1.5 with prev", at(1.5, Some(&prev)))] {
            let (x, y) = xy(&i, 1);
            assert!((x - is).abs() < 1e-5 && (y - centre(1, 0).y).abs() < 1e-5, "{label}: the path character sits at centre + sub: ({x}, {y}) vs ({is}, {})", centre(1, 0).y);
        }
        // Blend 0.5: half-way between the two EXACT positions.
        let (x, _) = xy(&at(0.5, Some(&prev)), 1);
        assert!((x - (was + is) * 0.5).abs() < 1e-5, "mid-tick the path character is half-way between where it was ({was}) and is ({is}): {x}");
        // Blend 0: where it WAS — the live cell, never the trail at (3,0).
        let (x, _) = xy(&at(0.0, Some(&prev)), 1);
        assert!((x - was).abs() < 1e-5, "at blend 0 the slide starts where the character exactly was ({was}), not at its trail ({}): {x}", centre(3, 0).x);
        // The teleport: at its new cell at every blend. A cut, exactly.
        for blend in [0.0, 0.5, 1.0] {
            let (x, y) = xy(&at(blend, Some(&prev)), 2);
            assert_eq!((x, y), (centre(3, 1).x, centre(3, 1).y), "a teleport (no ACTIVE_PATH) is drawn at its NEW cell at blend {blend}, never between: it would smear the scatter across the grid");
        }
        // The trail: drawn where it is, at its own remainder, at every blend.
        for blend in [0.0, 0.5] {
            let (x, _) = xy(&at(blend, Some(&prev)), 0);
            assert!((x - was).abs() < 1e-5, "a trail never moves (blend {blend}): {x} vs {was}");
        }
    }

    /// `Exact` never interpolates, and it agrees with `Slide` byte-for-byte once the
    /// tick completes — the two are not a smoothing stack, and a tile is never smoothed
    /// twice. Also the pin that `Slide` really does something at blend 0.5.
    #[test]
    fn t12_exact_never_interpolates_and_equals_slide_once_the_tick_completes() {
        let (prev, cur) = fixture();
        let reference = lower_opts(&cur, Some(&prev), 1.0, LowerOptions::default());
        for (label, got) in [
            ("Slide, blend 1, no prev", lower_opts(&cur, None, 1.0, LowerOptions::default())),
            ("Exact, blend 0.5, with prev", lower_opts(&cur, Some(&prev), 0.5, EXACT)),
            ("Exact, blend 0, with prev", lower_opts(&cur, Some(&prev), 0.0, EXACT)),
            ("Exact, blend 1, no prev", lower_opts(&cur, None, 1.0, EXACT)),
        ] {
            assert_eq!(got.0, reference.0, "{label}: instances must equal Slide at blend 1 — Exact interpolated, or Slide at blend 1 still saw prev");
            assert_eq!(got.1, reference.1, "{label}: tints");
            assert_eq!(got.2, reference.2, "{label}: emits");
            assert_eq!((got.3.min, got.3.max), (reference.3.min, reference.3.max), "{label}: bounds");
        }
        let slid = lower_opts(&cur, Some(&prev), 0.5, LowerOptions::default());
        assert_ne!(slid.0, reference.0, "the fixture has a path character with a remainder, so Slide at blend 0.5 must differ from blend 1");
        // Dark tiles are indifferent to the motion: they never slide under any variant.
        let dark_exact = LowerOptions { dark_tiles: true, motion: Motion::Exact };
        let (i_slide, _, e_slide, _) = lower_opts(&cur, Some(&prev), 0.5, DARK);
        let (i_exact, _, e_exact, _) = lower_opts(&cur, Some(&prev), 0.5, dark_exact);
        let darks = |i: &[Mat4], e: &[Vec4]| i.iter().zip(e).filter(|(_, e)| e.truncate() == glam::Vec3::ZERO).map(|(m, _)| *m).collect::<Vec<_>>();
        assert_eq!(darks(&i_slide, &e_slide), darks(&i_exact, &e_exact), "dark tiles are byte-identical under Slide and Exact");
        assert_eq!(i_exact.len(), i_slide.len());
    }

    /// `Cells` is the terminal's own picture: the remainder ignored and nothing sliding
    /// — byte-identical to lowering the same grid with every remainder zeroed and no
    /// previous grid. And it differs from `Slide` on a grid that carries remainders.
    #[test]
    fn t12_cells_ignores_the_remainder_and_never_slides() {
        let (prev, cur) = fixture();
        let zeroed = GlyphGrid { cells: cur.cells.iter().map(|c| GlyphCell { sub_x: 0.0, sub_y: 0.0, ..*c }).collect(), ..cur.clone() };
        for opts in [CELLS, LowerOptions { dark_tiles: true, motion: Motion::Cells }] {
            let reference = lower_opts(&zeroed, None, 1.0, LowerOptions { motion: Motion::Slide, ..opts });
            for blend in [0.0, 0.5, 1.0] {
                let got = lower_opts(&cur, Some(&prev), blend, opts);
                assert_eq!(got.0, reference.0, "Cells at blend {blend} (dark {}): every tile at its cell centre, nothing slid", opts.dark_tiles);
                assert_eq!(got.2, reference.2);
                assert_eq!((got.3.min, got.3.max), (reference.3.min, reference.3.max));
            }
        }
        let settled = lower_opts(&cur, None, 1.0, LowerOptions::default());
        assert_ne!(lower_opts(&cur, None, 1.0, CELLS).0, settled.0, "the fixture carries remainders, so Cells and Slide must differ even when settled");
        // A grid with no remainder and no previous frame: the three variants agree.
        let cells = lower_opts(&zeroed, None, 1.0, CELLS);
        assert_eq!(cells.0, lower_opts(&zeroed, None, 1.0, EXACT).0);
        assert_eq!(cells.0, lower_opts(&zeroed, None, 1.0, LowerOptions::default()).0);
    }

    /// Invariant #4 at the switch and at the wire: the default is today's motion, and the
    /// proposed `Shared.glyph[15]` mapping sends `0`, an unwritten lane and garbage to it.
    #[test]
    fn t12_motion_defaults_to_slide_and_the_lane_maps_everything_else_to_it() {
        assert_eq!(Motion::default(), Motion::Slide);
        assert_eq!(LowerOptions::default().motion, Motion::Slide);
        assert_eq!(Motion::from_lane(0.0), Motion::Slide);
        assert_eq!(Motion::from_lane(1.0), Motion::Exact);
        assert_eq!(Motion::from_lane(2.0), Motion::Cells);
        assert_eq!(Motion::from_lane(0.4), Motion::Slide, "nearest integer");
        assert_eq!(Motion::from_lane(1.4), Motion::Exact, "nearest integer");
        assert_eq!(Motion::from_lane(1.6), Motion::Cells, "nearest integer");
        for garbage in [-1.0, 3.0, 7.0, 1e9, -1e9, f32::NAN, f32::INFINITY] {
            assert_eq!(Motion::from_lane(garbage), Motion::Slide, "a lane value of {garbage} draws today's picture");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;
    use std::path::PathBuf;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("organon-glyph-test-{}-{}.bin", std::process::id(), tag))
    }

    fn cell(sym: char, id: u32) -> GlyphCell {
        GlyphCell { symbol: sym as u32, character_id: id, sgr: SGR_HAS_FG, fg: pack_rgb(1, 2, 3), ..Default::default() }
    }

    fn grid(cols: u32, rows: u32, f: impl Fn(usize, usize) -> GlyphCell) -> (GlyphFrame, Vec<GlyphCell>) {
        let mut cells = Vec::with_capacity((cols * rows) as usize);
        for r in 0..rows as usize {
            for c in 0..cols as usize {
                cells.push(f(c, r));
            }
        }
        (GlyphFrame { cols, rows, ..Default::default() }, cells)
    }

    /// Writer and reader are separate binaries indexing one mmap by offset, so the
    /// layout is pinned. If this fails, move the new field to the tail — never edit the
    /// expected numbers.
    #[test]
    fn layout_is_pinned() {
        assert_eq!(std::mem::size_of::<GlyphCell>(), 32);
        assert_eq!(offset_of!(GlyphCell, symbol), 0);
        assert_eq!(offset_of!(GlyphCell, fg), 4);
        assert_eq!(offset_of!(GlyphCell, bg), 8);
        assert_eq!(offset_of!(GlyphCell, sgr), 12);
        assert_eq!(offset_of!(GlyphCell, layer), 16);
        assert_eq!(offset_of!(GlyphCell, character_id), 20);
        assert_eq!(offset_of!(GlyphCell, sub_x), 24);
        assert_eq!(offset_of!(GlyphCell, sub_y), 28);
        assert_eq!(std::mem::size_of::<GlyphFrame>(), 64);
        assert_eq!(offset_of!(GlyphFrame, seq), 0);
        assert_eq!(offset_of!(GlyphFrame, effect), 32);
        assert_eq!(std::mem::size_of::<GlyphRingHeader>(), 32);
        assert_eq!(offset_of!(GlyphRingHeader, write_seq), 24);
        assert_eq!(GR_FILE_BYTES, 32 + 2 * (64 + GR_MAX_CELLS * 32));
    }

    #[test]
    fn round_trip_and_orientation_with_an_asymmetric_fixture() {
        let p = tmp_path("rt");
        let _ = std::fs::remove_file(&p);
        // 3 wide, 2 tall, and NOT symmetric: top row "AB.", bottom row "C.."
        // (row 0 is the TOP row by the ring's contract).
        let (meta, cells) = grid(3, 2, |c, r| match (c, r) {
            (0, 0) => cell('A', 1),
            (1, 0) => cell('B', 2),
            (0, 1) => cell('C', 3),
            _ => GlyphCell::default(),
        });
        {
            let mut w = GlyphRingWriter::create_at(&p, 2.0, 30.0).unwrap();
            w.publish(&meta, &cells).unwrap();
        }
        let r = GlyphRingReader::open_at(&p);
        assert!(r.is_open());
        let g = r.latest().expect("one frame published");
        assert_eq!((g.cols(), g.rows()), (3, 2));
        assert_eq!(g.at(0, 0).symbol, 'A' as u32, "top-left is the first cell");
        assert_eq!(g.at(1, 0).symbol, 'B' as u32);
        assert_eq!(g.at(0, 1).symbol, 'C' as u32, "row 1 is the BOTTOM row");
        assert_eq!(g.at(2, 1).symbol, 0);
        assert_eq!(g.cell_aspect, 2.0);
        assert_eq!(g.tick_hz, 30.0);
        assert_eq!(g.frame.seq, 1);
        assert_eq!(g.frame.generation, 1);
        assert_eq!(unpack_rgb(g.at(0, 0).fg), [1, 2, 3]);
        let _ = std::fs::remove_file(&p);
    }

    /// §7: the sub-cell offset survives the ring byte for byte (it is stored as the
    /// `f32` pair it is, no quantisation), a whole-cell position is exactly zero on both
    /// axes, and the two axes are not confused on the way through.
    #[test]
    fn the_sub_cell_offset_round_trips_and_a_whole_cell_position_is_zero() {
        let p = tmp_path("sub");
        let _ = std::fs::remove_file(&p);
        let (meta, cells) = grid(2, 1, |c, _| match c {
            0 => GlyphCell { sub_x: 0.25, sub_y: -0.5, ..cell('A', 1) },
            _ => cell('B', 2),
        });
        {
            let mut w = GlyphRingWriter::create_at(&p, 2.0, 30.0).unwrap();
            w.publish(&meta, &cells).unwrap();
        }
        let g = GlyphRingReader::open_at(&p).latest().expect("published");
        assert_eq!((g.at(0, 0).sub_x, g.at(0, 0).sub_y), (0.25, -0.5), "exact: x is x and y is y");
        assert_eq!((g.at(1, 0).sub_x, g.at(1, 0).sub_y), (0.0, 0.0), "a placed character has no remainder");
        assert_eq!(g.cells, cells);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn generation_bumps_only_when_the_cells_change() {
        let p = tmp_path("gen");
        let _ = std::fs::remove_file(&p);
        let (meta, cells) = grid(2, 1, |c, _| cell(if c == 0 { 'x' } else { 'y' }, c as u32));
        let mut w = GlyphRingWriter::create_at(&p, 2.0, 60.0).unwrap();
        w.publish(&meta, &cells).unwrap();
        w.publish(&meta, &cells).unwrap(); // a dwell heartbeat — same payload
        let r = GlyphRingReader::open_at(&p);
        let g = r.latest().unwrap();
        assert_eq!(g.frame.seq, 2, "every publish advances seq");
        assert_eq!(g.frame.generation, 1, "an identical payload keeps its generation");
        let mut moved = cells.clone();
        moved.swap(0, 1);
        w.publish(&meta, &moved).unwrap();
        assert_eq!(r.latest().unwrap().frame.generation, 2, "a changed payload bumps it");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn the_reader_refuses_a_ring_of_another_layout() {
        let p = tmp_path("layout");
        let _ = std::fs::remove_file(&p);
        let (meta, cells) = grid(1, 1, |_, _| cell('z', 9));
        {
            let mut w = GlyphRingWriter::create_at(&p, 2.0, 60.0).unwrap();
            w.publish(&meta, &cells).unwrap();
        }
        // Corrupt `cell_bytes` in place: a writer built against a different cell.
        {
            let f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
            let mut m = unsafe { memmap2::MmapMut::map_mut(&f).unwrap() };
            let off = offset_of!(GlyphRingHeader, cell_bytes);
            m[off..off + 4].copy_from_slice(&40u32.to_le_bytes());
        }
        let r = GlyphRingReader::open_at(&p);
        assert!(r.is_open());
        assert!(r.latest().is_none(), "a stride disagreement must read as NO signal");
        assert!(r.seq().is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_torn_slot_is_rejected() {
        let p = tmp_path("torn");
        let _ = std::fs::remove_file(&p);
        let (meta, cells) = grid(1, 1, |_, _| cell('z', 9));
        {
            let mut w = GlyphRingWriter::create_at(&p, 2.0, 60.0).unwrap();
            w.publish(&meta, &cells).unwrap();
        }
        // The slot's own seq disagreeing with the header is a slot the writer is mid-way
        // through — it must not be handed on.
        {
            let f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
            let mut m = unsafe { memmap2::MmapMut::map_mut(&f).unwrap() };
            let o = slot_offset(0);
            m[o..o + 4].copy_from_slice(&77u32.to_le_bytes());
        }
        assert!(GlyphRingReader::open_at(&p).latest().is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn an_absent_ring_is_silent_and_a_grid_too_big_is_refused() {
        let p = tmp_path("absent");
        let _ = std::fs::remove_file(&p);
        let r = GlyphRingReader::open_at(&p);
        assert!(!r.is_open());
        assert!(r.latest().is_none());
        let mut w = GlyphRingWriter::create_at(&p, 2.0, 60.0).unwrap();
        let too_many = vec![GlyphCell::default(); GR_MAX_CELLS + 1];
        let meta = GlyphFrame { cols: (GR_MAX_CELLS + 1) as u32, rows: 1, ..Default::default() };
        assert!(w.publish(&meta, &too_many).is_err(), "never truncate a grid silently");
        let (meta, cells) = grid(2, 2, |_, _| GlyphCell::default());
        assert!(w.publish(&meta, &cells[..3]).is_err(), "cols*rows must equal cells.len()");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn two_namespaces_are_two_rings() {
        let ns_a = format!("glyph-a-{}", std::process::id());
        let ns_b = format!("glyph-b-{}", std::process::id());
        for ns in [&ns_a, &ns_b] {
            let _ = std::fs::remove_file(crate::ipc::glyph_ring_path_in(ns).unwrap());
        }
        let (meta, a) = grid(1, 1, |_, _| cell('a', 1));
        let (_, b) = grid(1, 1, |_, _| cell('b', 2));
        {
            let mut wa = GlyphRingWriter::create_ns(&ns_a, 2.0, 60.0).unwrap();
            let mut wb = GlyphRingWriter::create_ns(&ns_b, 2.0, 60.0).unwrap();
            wa.publish(&meta, &a).unwrap();
            wb.publish(&meta, &b).unwrap();
        }
        let ga = GlyphRingReader::open_ns(&ns_a).unwrap().latest().unwrap();
        let gb = GlyphRingReader::open_ns(&ns_b).unwrap().latest().unwrap();
        assert_eq!(ga.at(0, 0).symbol, 'a' as u32);
        assert_eq!(gb.at(0, 0).symbol, 'b' as u32);
        assert!(GlyphRingReader::open_ns("../evil").is_err());
        assert!(GlyphRingWriter::create_ns("a b", 2.0, 60.0).is_err());
        for ns in [&ns_a, &ns_b] {
            let _ = std::fs::remove_file(crate::ipc::glyph_ring_path_in(ns).unwrap());
        }
    }

    #[test]
    fn frame_names_round_trip_and_truncate_on_a_char_boundary() {
        let mut f = GlyphFrame::default();
        set_frame_name(&mut f, "rain");
        assert_eq!(frame_name(&f), "rain");
        set_frame_name(&mut f, &"é".repeat(40)); // 80 bytes, 2 per char
        assert_eq!(frame_name(&f).chars().count(), 16);
    }

    /// T11: the trail bit survives the ring, is its own power of two, and shares no bit
    /// with the presence / path bits it is read beside. `FRAME_SETTLED` lives in a
    /// different word (`GlyphFrame.flags`), so a collision there is impossible by
    /// construction — it is in the list so that the list is the whole flag set.
    #[test]
    fn the_persist_flag_round_trips_and_shares_no_bit() {
        let p = tmp_path("persist");
        let _ = std::fs::remove_file(&p);
        let (meta, cells) = grid(2, 1, |c, _| match c {
            0 => GlyphCell { sgr: SGR_HAS_FG | SGR_PERSIST, ..cell('▀', 4) },
            _ => cell('█', 5),
        });
        {
            let mut w = GlyphRingWriter::create_at(&p, 2.0, 30.0).unwrap();
            w.publish(&meta, &cells).unwrap();
        }
        let g = GlyphRingReader::open_at(&p).latest().expect("published");
        assert_eq!(g.at(0, 0).sgr, SGR_HAS_FG | SGR_PERSIST, "the bit is read back exactly");
        assert_eq!(g.at(1, 0).sgr & SGR_PERSIST, 0, "…and only where it was written");
        assert_eq!(g.at(0, 0).symbol, '▀' as u32, "a trail keeps its symbol through the ring");
        let _ = std::fs::remove_file(&p);
        let sgr_bits = [
            SGR_BOLD, SGR_DIM, SGR_ITALIC, SGR_UNDERLINE, SGR_BLINK, SGR_REVERSE, SGR_HIDDEN,
            SGR_STRIKE, SGR_HAS_FG, SGR_HAS_BG, SGR_ACTIVE_PATH, SGR_PERSIST,
        ];
        for (i, a) in sgr_bits.iter().enumerate() {
            assert_eq!(a.count_ones(), 1, "bit {i} is a single bit");
            for b in &sgr_bits[i + 1..] {
                assert_eq!(a & b, 0, "SGR bits collide: {a:#x} & {b:#x}");
            }
        }
        assert_eq!(SGR_PERSIST & (SGR_HAS_FG | SGR_HAS_BG | SGR_ACTIVE_PATH), 0);
        assert_eq!(FRAME_SETTLED.count_ones(), 1);
    }

    /// T11 decays in linear light and publishes sRGB8: the encoder must be the exact
    /// inverse of the decoder on every one of the 256 codes, or a colour that merely
    /// passed through linear would publish a different byte from the one it arrived as.
    #[test]
    fn srgb_encode_round_trips_every_code_exactly_and_is_monotone() {
        for v in 0..=255u8 {
            assert_eq!(linear_to_srgb8(srgb8_to_linear(v)), v, "code {v}");
        }
        assert_eq!(linear_to_srgb8(0.0), 0);
        assert_eq!(linear_to_srgb8(1.0), 255);
        assert_eq!(linear_to_srgb8(2.0), 255, "clamped, never wrapped");
        assert_eq!(linear_to_srgb8(-1.0), 0);
        assert_eq!(linear_to_srgb8(f32::NAN), 0);
        let mut last = 0;
        for i in 0..=1000 {
            let b = linear_to_srgb8(i as f32 / 1000.0);
            assert!(b >= last, "monotone: {i}");
            last = b;
        }
        // The classic direction of the gamma bug, from this side: a mid-grey byte fed
        // to the encoder AS IF it were linear comes out far brighter than it went in.
        assert!(linear_to_srgb8(128.0 / 255.0) > 180, "{}", linear_to_srgb8(128.0 / 255.0));
        assert_eq!(pack_linear_rgb([1.0, 0.0, srgb8_to_linear(77)]), pack_rgb(255, 0, 77));
    }

    #[test]
    fn srgb_decode_is_the_iec_curve() {
        assert_eq!(srgb8_to_linear(0), 0.0);
        assert!((srgb8_to_linear(255) - 1.0).abs() < 1e-6);
        // Mid-grey: 128/255 = 0.502 encoded → ~0.2158 linear. Skipping the decode would
        // read it as 0.502, i.e. ~2.3× too bright — §4's classic bug, pinned here.
        let mid = srgb8_to_linear(128);
        assert!((mid - 0.2158).abs() < 1e-3, "{mid}");
        assert!(0.502 / mid > 2.0);
        assert_eq!(unpack_rgb(pack_rgb(10, 20, 30)), [10, 20, 30]);
    }

    #[test]
    fn block_glyphs_map_to_sub_cell_rectangles_and_shades_to_depth() {
        let full = tile_for('█' as u32).unwrap();
        assert_eq!((full.x0, full.y0, full.x1, full.y1, full.depth, full.emission), (0.0, 0.0, 1.0, 1.0, 1.0, 1.0));
        let upper = tile_for('▀' as u32).unwrap();
        assert_eq!((upper.y0, upper.y1), (0.5, 1.0), "upper half sits in the TOP half (y up)");
        let lower = tile_for('▄' as u32).unwrap();
        assert_eq!((lower.y0, lower.y1), (0.0, 0.5));
        assert_eq!(tile_for('▁' as u32).unwrap().y1, 0.125);
        assert_eq!(tile_for('▏' as u32).unwrap().x1, 0.125);
        assert_eq!(tile_for('▐' as u32).unwrap().x0, 0.5);
        assert_eq!(tile_for('░' as u32).unwrap().depth, 0.25);
        assert_eq!(tile_for('▒' as u32).unwrap().depth, 0.5);
        assert_eq!(tile_for('▓' as u32).unwrap().depth, 0.75);
        assert_eq!(tile_for(' ' as u32), None);
        assert_eq!(tile_for(0), None);
        // The unknown rule: a letter is a full block at reduced emission — visible, not
        // faithful, and distinguishable from a real block.
        let a = tile_for('A' as u32).unwrap();
        assert_eq!((a.x0, a.x1, a.y0, a.y1), (0.0, 1.0, 0.0, 1.0));
        assert!(a.emission < 1.0 && a.depth < 1.0);
    }

    // ── The blend clock (T12's finding) ──────────────────────────────────────────

    fn frame_at(epoch: u32, tick: u32) -> GlyphFrame {
        GlyphFrame { epoch, tick, cols: 1, rows: 1, ..Default::default() }
    }

    /// A display over a producer, on one ideal clock: tick `k` is published at `k / hz`
    /// (`k ≥ 1`), a frame is built at `phase + j / fps`, and each frame reads the newest
    /// published tick if it changed — exactly the world's loop. Returns, per frame from
    /// the first tick read, `(blend, drawn position in ticks)` for a character moving
    /// one cell a tick: `prev + blend × (cur − prev)`, which is what `lower_grid` draws.
    fn schedule(hz: f64, fps: f64, phase: f64, frames: usize) -> Vec<(f32, f64)> {
        let mut clock = BlendClock::default();
        let mut cur: Option<GlyphFrame> = None;
        let mut prev_tick = 0u32;
        let mut out = Vec::new();
        for j in 0..frames {
            let now = phase + j as f64 / fps;
            let newest = (now * hz + 1e-9).floor() as u32;
            if newest >= 1 && cur.map_or(true, |c| c.tick != newest) {
                let new = frame_at(0, newest);
                if clock.arrive(cur.as_ref(), &new, hz as f32, now) != Arrival::Heartbeat {
                    prev_tick = cur.map_or(newest, |c| c.tick);
                }
                cur = Some(new);
            }
            let Some(c) = cur else { continue };
            let blend = clock.blend(now);
            out.push((blend, prev_tick as f64 + blend as f64 * (c.tick - prev_tick) as f64));
        }
        out
    }

    fn steps(s: &[(f32, f64)]) -> Vec<f64> {
        s.windows(2).map(|w| w[1].1 - w[0].1).collect()
    }

    /// T12's case, the producer's default over an ordinary display. The old clock drew
    /// every frame at `blend ≈ 0` — the grid read one frame earlier, two ticks behind —
    /// because it measured from the read and evaluated at build time. With the lead the
    /// blend is 1 on every frame: the newest sample, two ticks a frame, uniform.
    #[test]
    fn a_120hz_producer_on_a_60hz_display_draws_the_newest_sample_every_frame() {
        // Phase a quarter-tick past a publish, so the read is never exactly on one.
        let s = schedule(120.0, 60.0, 1.0 / 480.0, 12);
        // The first read is a cut (nothing to slide from); every frame after it is a
        // pair two ticks apart, read at its start, shown a frame later.
        for (j, (blend, _)) in s.iter().enumerate().skip(1) {
            assert_eq!(*blend, 1.0, "120/60 frame {j}: blend {blend} — drawn behind the newest sample");
        }
        for (j, st) in steps(&s).iter().enumerate().skip(1) {
            assert!((st - 2.0).abs() < 1e-9, "120/60 step {j}: {st} ticks a frame, not 2");
        }
        // And the arithmetic of the finding itself, as one number each way: a two-tick
        // pair read `since ≈ 0`, with no lead, is the old answer; with the frame's own
        // interval as the lead it is the new one.
        assert_eq!(blend_for(0.0, 0.0, Some(1.0 / 60.0)), 0.0, "the old clock: 0 at every read");
        assert_eq!(blend_for(0.0, 1.0 / 60.0, Some(1.0 / 60.0)), 1.0, "the lead closes it");
    }

    /// The other direction: the display outruns the producer, and the four frames
    /// between two ticks are four interpolated positions — 0.25, 0.5, 0.75, 1.0 —
    /// reaching the newest sample exactly as the next tick lands. Quarter-tick steps.
    #[test]
    fn a_30hz_producer_on_a_120hz_display_gets_four_interpolated_frames() {
        // Frames 0..4 precede the first publish (tick 1 lands at 33 ms) and are not
        // reported; frames 4..8 are that first tick (a cut, blend 1); from the second
        // tick on, the quartet repeats.
        let s = schedule(30.0, 120.0, 0.0, 4 + 4 + 4 * 3);
        let blends: Vec<f32> = s.iter().skip(4).map(|b| b.0).collect();
        assert_eq!(blends, [0.25, 0.5, 0.75, 1.0, 0.25, 0.5, 0.75, 1.0, 0.25, 0.5, 0.75, 1.0], "30/120 blends: {blends:?}");
        for (j, st) in steps(&s[4..]).iter().enumerate() {
            assert!((st - 0.25).abs() < 1e-9, "30/120 step {j}: {st} ticks a frame, not 0.25");
        }
    }

    /// Equal rates: the old clock drew one tick behind (a fresh grid every frame at
    /// `since ≈ 0`); now the sample itself, one tick a frame.
    #[test]
    fn equal_rates_draw_the_sample_itself() {
        let s = schedule(60.0, 60.0, 1.0 / 240.0, 10);
        for (j, (blend, _)) in s.iter().enumerate().skip(1) {
            assert_eq!(*blend, 1.0, "60/60 frame {j}: blend {blend}");
        }
        for st in steps(&s).iter().skip(1) {
            assert!((st - 1.0).abs() < 1e-9, "60/60 step: {st}");
        }
    }

    /// A ratio that is `n:1` in neither direction. `Δtick` per read alternates 1, 2 and
    /// the lead does not, yet the drawn step is 1.5 ticks a frame throughout: a one-tick
    /// pair lands on its sample and a two-tick pair three quarters along it.
    #[test]
    fn a_90hz_producer_on_a_60hz_display_steps_evenly() {
        let s = schedule(90.0, 60.0, 1.0 / 360.0, 14);
        for (j, st) in steps(&s).iter().enumerate().skip(2) {
            assert!((st - 1.5).abs() < 1e-9, "90/60 step {j}: {st} ticks a frame, not 1.5");
        }
    }

    /// The settle publish and every dwell heartbeat republish at the same `tick`. They
    /// replace the picture (a T11 trail decays; `generation` may move) and they must not
    /// touch the clock: a slide in progress runs to completion under them, and one that
    /// has completed stays at 1. T5 and T11 depend on a heartbeat moving nothing.
    #[test]
    fn heartbeat_does_not_restart_the_clock() {
        let hz = 30.0;
        let dt = 1.0 / 120.0;
        let mut c = BlendClock::default();
        let f1 = frame_at(0, 1);
        assert_eq!(c.arrive(None, &f1, hz, 0.0), Arrival::Cut);
        assert_eq!(c.blend(0.0), 1.0, "a cut draws exactly");
        let f2 = frame_at(0, 2);
        assert_eq!(c.arrive(Some(&f1), &f2, hz, dt), Arrival::Tick(1.0 / 30.0));
        assert_eq!(c.blend(dt), 0.25);
        // The settle publish, 10 ms into the pair: same tick, different payload.
        let settle = GlyphFrame { flags: FRAME_SETTLED, generation: 9, ..f2 };
        assert_eq!(c.arrive(Some(&f2), &settle, hz, dt + 0.010), Arrival::Heartbeat);
        assert_eq!(c.period(), Some(1.0 / 30.0), "the pair is still the pair");
        assert_eq!(c.blend(2.0 * dt), 0.5, "a heartbeat must not restart the blend clock: the slide from tick 1 to tick 2 is half-way at +16.7 ms whether or not a settle frame arrived at +18 ms");
        assert_eq!(c.blend(3.0 * dt), 0.75);
        assert_eq!(c.blend(4.0 * dt), 1.0);
        // Dwell heartbeats every 250 ms after the slide completed: held at 1.
        let mut last = settle;
        for k in 1..=4 {
            let hb = GlyphFrame { generation: 9 + k, ..last };
            let t = 4.0 * dt + 0.25 * k as f64;
            assert_eq!(c.arrive(Some(&last), &hb, hz, t), Arrival::Heartbeat);
            assert_eq!(c.blend(t), 1.0, "heartbeat {k}: held at the settled grid");
            last = hb;
        }
    }

    /// A producer that stops mid-slide: the blend clamps at 1 and holds at the newest
    /// sample, however long the silence — never past it.
    #[test]
    fn a_stalled_producer_clamps_at_one() {
        let mut c = BlendClock::default();
        let f1 = frame_at(0, 1);
        c.arrive(None, &f1, 30.0, 0.0);
        c.blend(0.0);
        c.arrive(Some(&f1), &frame_at(0, 2), 30.0, 0.01);
        let b = c.blend(0.01);
        assert!((b - 0.3).abs() < 1e-6, "0.3 = the 10 ms lead over a 33 ms pair: {b}");
        for t in [0.05, 0.1, 1.0, 10.0, 3600.0] {
            let b = c.blend(t);
            assert_eq!(b, 1.0, "at +{t} s: {b}");
        }
        assert_eq!(blend_for(100.0, 100.0, Some(0.01)), 1.0, "never overshoots");
        assert_eq!(blend_for(-1.0, -1.0, Some(0.01)), 0.0, "negative time is 0, not a NaN");
        assert_eq!(blend_for(0.5, 0.0, Some(0.0)), 1.0, "a zero period is a cut");
    }

    #[test]
    fn arrivals_are_classified_by_epoch_and_tick() {
        let cur = frame_at(3, 10);
        assert_eq!(classify_arrival(None, &cur, 120.0), Arrival::Cut, "the first frame");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(4, 1), 120.0), Arrival::Cut, "a new effect");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(4, 11), 120.0), Arrival::Cut, "a new effect is a cut whatever its tick says");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(4, 10), 120.0), Arrival::Cut, "a new effect at the same tick is a cut, not a heartbeat");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(3, 7), 120.0), Arrival::Cut, "a restarted producer");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(3, 10), 120.0), Arrival::Heartbeat);
        assert_eq!(classify_arrival(Some(&cur), &frame_at(3, 11), 120.0), Arrival::Tick(1.0 / 120.0));
        assert_eq!(classify_arrival(Some(&cur), &frame_at(3, 12), 120.0), Arrival::Tick(1.0 / 60.0), "two ticks apart is a two-tick pair");
        assert_eq!(classify_arrival(Some(&cur), &frame_at(3, 11), 0.0), Arrival::Tick(1.0 / 60.0), "a header with no rate falls back to 60, as the world always has");
        // A cut leaves the clock with no period, and the next tick starts one.
        let mut c = BlendClock::default();
        c.arrive(Some(&cur), &frame_at(4, 1), 120.0, 5.0);
        assert_eq!(c.period(), None);
        assert_eq!(c.blend(5.0), 1.0);
        c.arrive(Some(&frame_at(4, 1)), &frame_at(4, 2), 120.0, 5.0 + 1.0 / 120.0);
        assert_eq!(c.period(), Some(1.0 / 120.0));
    }
}
