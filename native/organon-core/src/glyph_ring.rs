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
//! `sub_x` / `sub_y` are RESERVED: the pre-rounded sub-cell offset §7 describes, which
//! nothing writes yet. They are in the layout on day one because widening a ring is
//! cheap now and expensive on day thirty.

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
    /// RESERVED (§6.1 / §7): sub-cell offset in cell units, `+x` right, `+y` up, from the
    /// cell's centre. Always `0.0` today; a producer carrying the pre-rounded path point
    /// will fill it, and a consumer that adds it now is already correct then.
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
    /// Effect frame index, 0-based, reset when a new effect starts.
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
/// **Motion (§7).** A cell whose `SGR_ACTIVE_PATH` bit is set and whose `character_id`
/// sat in a *different* cell of `prev` is drawn at `lerp(prev_centre, centre, blend)`:
/// the character is sliding along a path, so smoothing between the two integer cells
/// changes where it is *between* ticks and never when it arrives. A cell without the
/// bit was placed by `set_coordinate` — a cut — and is drawn where it is; interpolating
/// it would invent motion the effect never authored. `blend` is `0..=1`, the caller's
/// `elapsed / (1 / tick_hz)`.
///
/// Extent, depth and position are all in cell units scaled by `look.cell_w`; the row
/// pitch honours `grid.cell_aspect` (§7: 2:1, or every ring becomes an ellipse).
pub fn lower_grid(grid: &GlyphGrid, prev: Option<&GlyphGrid>, blend: f32, look: &GlyphLook, out: TileOut<'_>) -> crate::math::Bounds {
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
    // Where each character was last frame, for the slide. Built only when a previous
    // grid exists and the blend is not already complete (then the answer is `centre`).
    let prev_pos: std::collections::HashMap<u32, (usize, usize)> = match prev {
        Some(p) if blend < 1.0 && p.cols() == cols && p.rows() == rows => {
            let mut m = std::collections::HashMap::with_capacity(p.cells.len() / 4 + 1);
            for (i, cell) in p.cells.iter().enumerate() {
                if cell.symbol != 0 && tile_for(cell.symbol).is_some() {
                    m.insert(cell.character_id, (i % cols, i / cols));
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
            let Some(tile) = tile_for(cell.symbol) else { continue };
            let mut centre = cell_centre(c, r, cols, rows, look, aspect);
            if cell.sgr & SGR_ACTIVE_PATH != 0 {
                if let Some(&(pc, pr)) = prev_pos.get(&cell.character_id) {
                    if (pc, pr) != (c, r) {
                        let from = cell_centre(pc, pr, cols, rows, look, aspect);
                        centre = from.lerp(centre, blend);
                    }
                }
            }
            // Reserved sub-cell offset (§6.1): honoured now so a producer that starts
            // writing it needs no consumer change. Zero today.
            centre.x += cell.sub_x * w;
            centre.y += cell.sub_y * h;
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
            bounds.min = bounds.min.min(pos - Vec3::new(sx, sy, sz) * 0.5);
            bounds.max = bounds.max.max(pos + Vec3::new(sx, sy, sz) * 0.5);
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

    #[test]
    fn an_empty_grid_lowers_to_nothing() {
        let g = grid_of(0, 0, vec![]);
        let (i, ..) = lower(&g, None, 1.0);
        assert!(i.is_empty());
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
}
