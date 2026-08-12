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
//!
//! **Console Spike Tier 4 makes the backdrop a stack of bands rather than one quad.**
//! Every row shows the look that was active when the text on it was written, so a
//! `background` command applies *forward* and history keeps its own styling. Two pieces
//! live here: [`PaneAnchor`], which is the one place `dropped` is maintained (the whole
//! of [`crate::scroll_anchor`]'s caller contract), and [`band_quads`], which turns
//! (boundaries, [`scroll_anchor::ViewState`], rect) into the rects and UVs to paint. The
//! *looks* themselves — which epoch wears what, and which of them still own a texture —
//! belong to the caller (`shell_main.rs`, over `substrate_epochs`); this module never
//! learns what a material is.
//!
//! **Console Spike Tier 5 puts a live control panel in the rows a block reserved.**
//! [`crate::block_panel`] owns what that panel is; this module owns *when* it is drawn —
//! between the scrim and the glyph loop, so a panel is the one place the engine is not dimmed
//! and yet can never cover a character — and *whether the wheel belongs to it*, which is the
//! one place the terminal has to give something up. It is not a texture and there is no
//! second render pass: the console's whole frame is already one egui pass, so a block is a
//! child `Ui` at a rect.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use crate::block_panel::{self, BlockAction, BlockPanel};
use crate::scroll_anchor::{self, Snapshot, ViewState};
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

// ---------------------------------------------------------------------------
// The scroll anchor's caller side (Console Spike Tier 4)
// ---------------------------------------------------------------------------

/// One pane's place in [`crate::scroll_anchor`]'s absolute-line coordinate: the `dropped`
/// counter, and the last primary-screen geometry a boundary can be taken against.
///
/// **This type exists so that the counter has exactly one update site.** The contract in
/// `scroll_anchor`'s module doc says to bracket `session.pump()` — but a console pumps
/// every tab every frame *and* pumps the active tab again while drawing it
/// (`shell_main.rs`'s redraw loop, then [`draw`] below), so "one site" cannot mean "one
/// call". It means one **function**: `PaneAnchor::bracketed` is the only place
/// `advance_dropped` is ever called, every call site goes through it, and a bare
/// `session.pump()` anywhere in the console would be the bug. The bracket contains the
/// parser advance and nothing else — in particular not `scroll_display`, which moves
/// `display_offset` for a reason the counter would read as emitted output (checklist item 3).
///
/// **Console Spike Tier 5 gives it a second kind of advance.** [`PaneAnchor::feed_local`]
/// pushes bytes the console generated itself through the same `Processor::advance` — that is
/// how a run of rows is reserved in the transcript — and it needs the identical bracket for
/// the identical reason. `TermSession::feed_local` is `pub(crate)`, so it is unreachable from
/// the console except through this type.
///
/// One per session, because scrollback is per session. Cheap: one `u64` and the last
/// primary-screen [`ViewState`] beside its cursor row.
#[derive(Clone, Debug, Default)]
pub struct PaneAnchor {
    /// Lines evicted off the top of this pane's scrollback. 0 until the 10 000-line buffer
    /// fills; also the absolute index of the oldest retained line.
    dropped: u64,
    /// Checklist item 6: the last geometry seen on the **primary** screen, and the cursor
    /// row with it. While a full-screen application is up, the alt grid reports zero
    /// scrollback and its coordinates are not comparable, so a look change during `htop`
    /// opens its epoch where the next primary line will actually be written.
    last_primary: Option<(ViewState, i32)>,
}

impl PaneAnchor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lines this pane has dropped off the top of scrollback. Read for diagnostics; the
    /// [`ViewState`] built by [`PaneAnchor::view`] is what the arithmetic wants.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// **Advance the parser and the counter together.** The one update site; see the type's
    /// doc for why that phrasing is load-bearing.
    pub fn pump(&mut self, session: &mut TermSession) {
        self.bracketed(session, |s| {
            s.pump();
        });
    }

    /// Open a run of rows in the transcript by advancing the parser against bytes the
    /// **console itself** produced (Console Spike Tier 5), and return the **absolute line
    /// index of the first row the feed opens**.
    ///
    /// Build the bytes with [`crate::term::block_bytes`]; this function is deliberately
    /// generic over them, because what it owns is the *bracket*, not the sequence.
    ///
    /// # Why this exists rather than a bare `session.feed_local`
    ///
    /// The same reason [`PaneAnchor::pump`] exists. Locally fed bytes go through
    /// `Processor::advance` exactly as the child's do, so they can push lines into history
    /// and — at the 10 000-line cap, with the user scrolled back — evict lines off the top.
    /// An unbracketed feed raises the true `screen_top` without raising the derived one:
    /// `advance_dropped` never sees it, and **every absolute index recorded before the feed
    /// is permanently wrong**, silently, for the life of the session. `TermSession::feed_local`
    /// is `pub(crate)` so that no caller outside this crate can make that mistake, and this
    /// is the only call to it.
    ///
    /// # The returned index, and where it comes from
    ///
    /// From the **pre-feed** [`ViewState`], by [`PaneAnchor::boundary_now`] — the line just
    /// below the cursor, in the same absolute coordinate a look-epoch boundary uses, on the
    /// same alt-screen-aware path. Absolute indices do not move when lines are emitted, so
    /// taking it before the feed and reading it after are the same number; taking it before
    /// is what makes it derivable at all.
    ///
    /// The identity the caller may rely on, given [`crate::term::block_bytes`]'s `rows + 1`
    /// linefeeds: the block occupies absolute lines `at ..= at + rows - 1`, and the cursor
    /// comes to rest on `at + rows`, one row below it.
    ///
    /// # What a reserved block cannot survive
    ///
    /// Stated the way [`crate::scroll_anchor`] states its own limits: these are design
    /// inputs, accepted, not defects awaiting a fix here.
    ///
    /// * **A width change reflows, and the anchor drifts.** Row and height resizes are exact
    ///   (`grid/resize.rs`, `grow_lines`/`shrink_lines` — `screen_top` and the cursor move
    ///   together). A *width* change re-wraps every wrapped line above the block, so the
    ///   block's absolute index slides by the net wrap delta; worse, `grow_columns` can drop
    ///   the block's topmost row outright when the line above it ended `WRAPLINE`, because a
    ///   `row.is_clear()` row is skipped rather than pushed (`grid/resize.rs:184-196`) — and a
    ///   reserved row is clear by construction. **The policy is that a width change
    ///   invalidates a block**: the console drops or re-derives it rather than painting a
    ///   stale rect. That policy is not implemented yet; this increment records it.
    /// * **Eviction erodes a block from the top, one row at a time**, and at the live edge
    ///   (`display_offset == 0`) evictions are not observable at all
    ///   (`scroll_anchor.rs`, "What this cannot do"), so the console can believe an eroded
    ///   block is still whole.
    /// * **`\x1b[3J` wipes the scrollback silently** — `Term` routes it to
    ///   `grid.clear_history()`, and `clear` / `reset` emit it. Nothing observes the loss.
    /// * **A resize while the alternate screen is up resizes the primary grid invisibly**
    ///   (`term/mod.rs:678`), so a block opened before `htop` can move while `htop` is on
    ///   screen.
    /// * **Feeding while the alt screen is up writes into the alt grid**, which has no
    ///   scrollback: the rows are opened in a live application's canvas and the returned
    ///   index — which comes from the last *primary* geometry — describes a row the feed did
    ///   not touch. A block asked for during a full-screen application is meaningless. The
    ///   caller is free to refuse one; this function does not, because refusing is policy.
    pub fn feed_local(&mut self, session: &mut TermSession, bytes: &[u8]) -> u64 {
        // Before the feed: the cursor has not moved yet, and this is the coordinate the
        // block is named by.
        let at = self.boundary_now(session);
        self.bracketed(session, |s| s.feed_local(bytes));
        at
    }

    /// The bracket itself: whatever `advance` does to the parser, `dropped` is updated across
    /// it and the primary-screen geometry is re-recorded after it.
    ///
    /// One function so that "exactly one update site" survives a second kind of advance.
    /// [`PaneAnchor::pump`] and [`PaneAnchor::feed_local`] differ only in which bytes reach
    /// `Processor::advance`; if they each carried their own copy of these six lines, the two
    /// copies would be one refactor away from disagreeing about what counts as an eviction.
    fn bracketed(&mut self, session: &mut TermSession, advance: impl FnOnce(&mut TermSession)) {
        let before = Snapshot {
            history: session.term.history_size(),
            display_offset: session.term.grid().display_offset(),
        };
        advance(session);
        let after = Snapshot {
            history: session.term.history_size(),
            display_offset: session.term.grid().display_offset(),
        };
        self.dropped = scroll_anchor::advance_dropped(self.dropped, before, after);

        // Item 6's bookkeeping, kept here so it cannot fall out of step with the counter:
        // every frame the pane is on the primary screen is a frame whose geometry a later
        // boundary may have to borrow.
        if !alt_screen(session) {
            self.last_primary = Some((self.view(session), cursor_row(session)));
        }
    }

    /// This pane's geometry, as plain integers. **Call it after the pump** (checklist item
    /// 5) — and after the wheel, so a scroll and the glyphs it moves agree within one frame.
    pub fn view(&self, session: &TermSession) -> ViewState {
        ViewState {
            rows: session.size().1,
            display_offset: session.term.grid().display_offset(),
            history: session.term.history_size(),
            dropped: self.dropped,
            alt_screen: alt_screen(session),
        }
    }

    /// Where a look change dispatched **now** should open its epoch: just below the cursor,
    /// in absolute lines (checklist item 6).
    ///
    /// On the alt screen this uses the last primary geometry instead of the live one. If a
    /// pane has never been seen on the primary screen — a harness that opens a full-screen
    /// application before its first frame — there is nothing better to use than the alt
    /// grid's own coordinates, and the ledger's monotonic clamp is what keeps the result
    /// ordered. That degradation is real and bounded to a session that started under `htop`.
    pub fn boundary_now(&self, session: &TermSession) -> u64 {
        let (state, cursor) = if alt_screen(session) {
            self.last_primary.unwrap_or_else(|| (self.view(session), cursor_row(session)))
        } else {
            (self.view(session), cursor_row(session))
        };
        scroll_anchor::boundary_now(state, cursor)
    }
}

/// `TermMode::ALT_SCREEN` — the one mode bit the band arithmetic cares about.
fn alt_screen(session: &TermSession) -> bool {
    session.term.mode().contains(TermMode::ALT_SCREEN)
}

/// The cursor's **grid** row (0 = top of the live screen), which is the frame of reference
/// [`scroll_anchor::boundary_now`] wants.
///
/// Read straight off the grid rather than through `renderable_content()`: that constructor
/// returns `term.grid.cursor.point` unchanged outside vi mode (which this terminal never
/// enters), and it also builds a display iterator, a selection range and a colour borrow
/// that a boundary has no use for.
fn cursor_row(session: &TermSession) -> i32 {
    session.term.grid().cursor.point.line.0
}

// ---------------------------------------------------------------------------
// The backdrop, as bands (Console Spike Tier 4)
// ---------------------------------------------------------------------------

/// The backdrop the caller wants painted: one boundary list, one texture per look-epoch.
///
/// **Presentation only.** This module does not know what a look is, cannot render one, and
/// never decides which epoch a row belongs to beyond the arithmetic in
/// [`crate::scroll_anchor`]. The caller owns the ledger and the textures; this is the slice
/// of it a painter needs.
///
/// Two shapes are load-bearing:
///
/// * `boundaries` is in [`crate::scroll_anchor`]'s form — the line at which each epoch
///   **after the first** opened, ascending. An epoch ledger that records a boundary for its
///   oldest epoch too must drop that first entry before handing it over; `textures.len()`
///   is what says whether it did.
/// * `textures.len() == boundaries.len() + 1`, oldest first, and `None` means **there is no
///   image of that look** — the backdrop was off while those rows were written, or they
///   predate this pane. Such a band paints nothing, so the panel's own background shows
///   through, which is exactly what "no backdrop" looked like at the time.
#[derive(Clone, Copy, Debug)]
pub struct BandedBackdrop<'a> {
    pub boundaries: &'a [u64],
    pub textures: &'a [Option<egui::TextureId>],
}

/// One band of backdrop, ready to paint: where it goes, which texture fills it, and which
/// slice of that texture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandQuad {
    pub rect: egui::Rect,
    /// The **UV slice**, not `0..1`. See [`band_quads`].
    pub uv: egui::Rect,
    /// `None` when no image of that epoch's look exists — paint nothing.
    pub texture: Option<egui::TextureId>,
}

/// Cut the grid rect into one quad per look-epoch.
///
/// # The UV policy, which is the whole visual argument
///
/// A cached epoch texture is a **full-pane image** of that look. A band showing it could
/// either (a) squeeze the whole pane into the band's height or (b) show the slice of it that
/// sits where the band sits. This does (b): `uv.y` spans the band's own fraction of the rect.
/// Under (a) a six-row band would compress a whole backdrop into six rows and the look would
/// visibly *change shape* as it scrolled; under (b) the substrate's gradient stays put and
/// the text scrolls over it, which is what "history keeps its own styling" has to look like
/// for the eye to read it as one surface rather than a stack of thumbnails.
///
/// The first band starts at `rect.top()` and the last ends at `rect.bottom()`, so the quads
/// partition the rect exactly — including the sub-cell remainder at the bottom that
/// `rows * cell_h` leaves over. With one epoch the result is exactly Tier 1's single
/// full-rect, `0..1` quad, which is what keeps `world`/`off` (and every frame before the
/// first `background` command) pixel-identical to the tier before this one.
pub fn band_quads(
    boundaries: &[u64],
    textures: &[Option<egui::TextureId>],
    state: ViewState,
    rect: egui::Rect,
    cell_h: f32,
) -> Vec<BandQuad> {
    let full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let newest = || textures.last().copied().flatten();

    let bands = scroll_anchor::bands(boundaries, state);
    // No rows to band (a pane mid-resize) or a degenerate cell height: one quad of the live
    // look, i.e. today's behaviour. A zero-height rect is not this function's to police.
    if bands.is_empty() || !(cell_h > 0.0) || rect.height() <= 0.0 {
        return vec![BandQuad { rect, uv: full, texture: newest() }];
    }

    let last = bands.len() - 1;
    let h = rect.height();
    bands
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let top = if i == 0 {
                rect.top()
            } else {
                (rect.top() + f32::from(b.first_vrow) * cell_h).min(rect.bottom())
            };
            let bottom = if i == last {
                rect.bottom()
            } else {
                (rect.top() + f32::from(b.last_vrow + 1) * cell_h).min(rect.bottom())
            };
            BandQuad {
                rect: egui::Rect::from_min_max(
                    egui::pos2(rect.left(), top),
                    egui::pos2(rect.right(), bottom.max(top)),
                ),
                uv: egui::Rect::from_min_max(
                    egui::pos2(0.0, (top - rect.top()) / h),
                    egui::pos2(1.0, (bottom.max(top) - rect.top()) / h),
                ),
                texture: textures.get(b.epoch).copied().flatten(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Cell metrics (Console Spike Tier 5)
// ---------------------------------------------------------------------------

/// The pane's font. One definition, because two constants named `14.0` in two files is how a
/// grid and the thing measuring it come to disagree by a row.
pub fn pane_font() -> egui::FontId {
    egui::FontId::monospace(14.0)
}

/// One cell's `(width, height)` in points, for a given font.
///
/// **A free function rather than something [`draw`] returns**, and the distinction is not
/// stylistic. Both numbers are a pure function of the [`egui::FontId`] and the context's font
/// definitions — they do not depend on the rect, the grid, or anything that happened this
/// frame — so a caller that needed `draw` to hand them back would be reading *last* frame's
/// measurement to make *this* frame's decision, and would be silently one frame stale across
/// exactly the events that matter (a font change, a DPI change, the first frame of all, when
/// there is no previous answer at all). Asking the fonts directly is always current.
///
/// [`draw`] calls this too, so exactly one definition of "how big is a cell" exists.
///
/// `glyph_width(&font, 'M')` is the monospace advance and `row_height` the line box: the same
/// pair the glyph loop steps by, which is what makes a rect derived from these land on cell
/// boundaries rather than near them.
pub fn cell_metrics(ctx: &egui::Context, font_id: &egui::FontId) -> (f32, f32) {
    ctx.fonts_mut(|f| (f.glyph_width(font_id, 'M'), f.row_height(font_id)))
}

/// One frame of the terminal: pump the session, size the grid to the rect, feed
/// input, paint. The caller gives us the whole window (PRD v3 §7.5: no chrome).
///
/// `backdrop` is tree E Tier 1 (the engine behind the glyphs): when `Some`, the
/// caller has already rendered the world into that texture this frame, and it is
/// painted under everything with the **legibility scrim** over it — PRD §4.6's
/// inviolable rule, enforced here structurally rather than by taste: whatever the
/// engine shows, the glyph layer keeps its contrast floor.
///
/// Console Spike Tier 4 makes that backdrop a [`BandedBackdrop`] — the live look at the
/// bottom, older looks above it, each band sampling its own epoch's texture. The scrim is
/// unchanged and still covers the whole rect, once, after every band.
///
/// `panels` is Tier 5: the runs of rows this pane reserved, each carrying a live egui control
/// panel drawn **over** the scrim (see the paint pass below for why that ordering is the whole
/// visual claim). Taken by `&mut` because a slider that cannot be moved is a picture of a
/// slider. An empty slice is the tier before this one, byte for byte.
///
/// The return value is the buttons a person pressed inside those panels this frame. This
/// module does not know what they mean — see [`crate::block_panel`] — so it hands them back to
/// the caller that supplied the labels.
pub fn draw(
    ui: &mut egui::Ui,
    session: &mut TermSession,
    anchor: &mut PaneAnchor,
    backdrop: Option<BandedBackdrop<'_>>,
    panels: &mut [BlockPanel],
) -> Vec<BlockAction> {
    let font_id = pane_font();
    let (cell_w, cell_h) = cell_metrics(ui.ctx(), &font_id);

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
    // The pump and the scroll-anchor counter, inseparably — see [`PaneAnchor`]. `resize`
    // above is deliberately OUTSIDE the bracket (it moves `history_size` without emitting a
    // line), and so is the wheel below (it moves `display_offset` without emitting one).
    anchor.pump(session);

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
    // ── The wheel, and the one thing a block takes from the terminal ───────
    // The console scrolls its transcript from **anywhere** in the window — there is no
    // scrollbar to be over — so a panel sitting inside the grid rect has to claim the pointer
    // explicitly or a slider drag would also scroll the block out from under the cursor.
    //
    // The placement is computed from the geometry as it stands *before* the wheel is applied,
    // and that is the right frame of reference rather than a convenient one: the pointer is
    // over what is on the screen right now, which is what this state describes. Applying the
    // wheel first and then asking would test the pointer against rows that have not been
    // drawn yet. `view` is five field reads, so computing it twice costs nothing.
    let pre_wheel = anchor.view(session);
    let blocks: Vec<crate::block_anchor::Block> = panels.iter().map(|p| p.block).collect();
    let placed = block_panel::placements(&blocks, pre_wheel, rect, cell_h);
    let pointer_on_panel =
        block_panel::pointer_inside(&placed, ui.input(|i| i.pointer.hover_pos()));
    let scroll = ui.input(|i| i.raw_scroll_delta.y);
    if scroll.abs() >= 1.0 && !pointer_on_panel {
        session.scroll_display((scroll / cell_h * 1.5) as i32);
    }

    // ── Paint ──────────────────────────────────────────────────────────────
    // Built here rather than beside the pump: this is after the wheel, so a scrolled
    // viewport's bands and its glyphs come from the same `display_offset` in the same frame.
    let state = anchor.view(session);
    let painter = ui.painter_at(rect);
    match backdrop {
        Some(bands) => {
            for quad in band_quads(bands.boundaries, bands.textures, state, rect, cell_h) {
                // A band with no image of its look paints nothing: the panel's own
                // background is showing through, which is what the backdrop looked like
                // when those rows were written.
                if let Some(texture) = quad.texture {
                    painter.image(texture, quad.rect, quad.uv, egui::Color32::WHITE);
                }
            }
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

    // ── Blocks (Console Spike Tier 5) ──────────────────────────────────────
    // **Here, and the position is the claim.** After the backdrop and after the scrim, so a
    // panel is the one place the engine is not dimmed — a window cut through the transcript
    // rather than another layer under it. Before the glyph loop and the cursor, so nothing a
    // panel draws can ever cover a character: text stays on top by construction, not by the
    // block's rows happening to be blank. They *are* blank (a reserved row is erased when it
    // is opened — `term::block_bytes`), so in practice nothing is overdrawn either way; the
    // ordering is what makes that a property instead of a coincidence.
    //
    // It is `state`, not `pre_wheel`, deliberately: the wheel has been applied by now, so the
    // panel is drawn where the glyphs of this same frame are, not where they were.
    let actions = block_panel::draw(ui, panels, state, rect, cell_h);

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

    actions
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

    // -------------------------------------------------------------------------
    // The banded backdrop (Console Spike Tier 4)
    // -------------------------------------------------------------------------

    /// A 400-point-tall pane of 20-point rows, the shape every band test below uses.
    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 30.0), egui::vec2(300.0, 400.0))
    }

    fn tex(n: u64) -> Option<egui::TextureId> {
        Some(egui::TextureId::User(n))
    }

    fn live(rows: u16, history: usize) -> ViewState {
        ViewState { rows, display_offset: 0, history, dropped: 0, alt_screen: false }
    }

    /// Bands must tile the rect with no gap and no overlap, top to bottom — a seam of one
    /// point would show as a dark line across the backdrop, and an overlap would double the
    /// scrim under it.
    fn assert_tiles(quads: &[BandQuad], rect: egui::Rect) {
        assert!(!quads.is_empty());
        assert_eq!(quads[0].rect.top(), rect.top(), "the first band must start at the top");
        assert_eq!(
            quads.last().unwrap().rect.bottom(),
            rect.bottom(),
            "the last band must reach the bottom"
        );
        for q in quads {
            assert_eq!(q.rect.left(), rect.left());
            assert_eq!(q.rect.right(), rect.right());
            assert!(q.rect.height() > 0.0, "empty band: {q:?}");
            // The UV slice is the band's own fraction of the rect — the policy the doc
            // argues for, as arithmetic.
            assert!((q.uv.top() - (q.rect.top() - rect.top()) / rect.height()).abs() < 1e-5);
            assert!(
                (q.uv.bottom() - (q.rect.bottom() - rect.top()) / rect.height()).abs() < 1e-5
            );
            assert_eq!((q.uv.left(), q.uv.right()), (0.0, 1.0), "bands are full-width");
        }
        for w in quads.windows(2) {
            assert_eq!(w[1].rect.top(), w[0].rect.bottom(), "gap or overlap: {w:?}");
        }
    }

    /// **Nothing changes until something changes.** One epoch — every frame before the first
    /// `background` command, and every frame at `world`/`off` — must be exactly Tier 1's
    /// single full-rect quad at UV 0..1, or this tier has quietly re-rendered the product's
    /// existing look.
    #[test]
    fn one_epoch_is_exactly_todays_single_full_rect_quad() {
        let r = pane();
        let quads = band_quads(&[], &[tex(7)], live(20, 500), r, 20.0);
        assert_eq!(
            quads,
            vec![BandQuad {
                rect: r,
                uv: egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                texture: tex(7),
            }]
        );
    }

    /// The Tier 4 beat, as geometry: two look changes inside the visible window cut it into
    /// three bands, each carrying its own epoch's texture, newest at the bottom.
    #[test]
    fn each_band_carries_its_own_epochs_texture() {
        let r = pane();
        // 20 rows of 20 points; the viewport shows absolute 500..520.
        let state = live(20, 500);
        let quads = band_quads(&[505, 512], &[tex(1), tex(2), tex(3)], state, r, 20.0);

        assert_eq!(quads.len(), 3);
        assert_eq!(
            quads.iter().map(|q| q.texture).collect::<Vec<_>>(),
            vec![tex(1), tex(2), tex(3)],
            "oldest look on top, live look at the bottom"
        );
        // Row 5 and row 12 are where the two changes landed.
        assert_eq!(quads[0].rect.bottom(), r.top() + 100.0);
        assert_eq!(quads[1].rect.bottom(), r.top() + 240.0);
        assert_tiles(&quads, r);
    }

    /// The bottom band absorbs the sub-cell remainder: 400 points of pane is 20 rows of
    /// 19.5, and the 10 points left over must not become an unpainted strip.
    #[test]
    fn the_last_band_absorbs_the_sub_cell_remainder() {
        let r = pane();
        let quads = band_quads(&[505], &[tex(1), tex(2)], live(20, 500), r, 19.5);
        assert_tiles(&quads, r);
        assert_eq!(quads[0].rect.height(), 5.0 * 19.5);
        assert_eq!(quads.last().unwrap().rect.bottom(), r.bottom());
    }

    /// A look with no image — the backdrop was off while those rows were written — paints
    /// nothing rather than borrowing the neighbouring look. The band is still emitted, so
    /// the tiling stays total.
    #[test]
    fn a_look_with_no_image_yields_a_band_with_no_texture() {
        let r = pane();
        let quads = band_quads(&[505], &[None, tex(2)], live(20, 500), r, 20.0);
        assert_eq!(quads[0].texture, None, "no image of that look");
        assert_eq!(quads[1].texture, tex(2));
        assert_tiles(&quads, r);
    }

    /// The alternate screen is a live application's canvas, not a transcript: one band of
    /// the live look, whatever the ledger holds. Guaranteed by the arithmetic rather than by
    /// a special case here — this pins that it stays that way.
    #[test]
    fn the_alt_screen_is_one_band_of_the_live_look() {
        let r = pane();
        let state = ViewState { alt_screen: true, ..live(20, 500) };
        let quads = band_quads(&[505, 512], &[tex(1), tex(2), tex(3)], state, r, 20.0);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].texture, tex(3));
        assert_eq!(quads[0].rect, r);
        assert_tiles(&quads, r);
    }

    /// Geometry this function must not divide by: a pane mid-resize (no rows), a font that
    /// measured to nothing. One quad of the live look — today's behaviour — never a panic
    /// and never a NaN UV.
    #[test]
    fn degenerate_geometry_falls_back_to_one_live_quad() {
        let r = pane();
        let textures = [tex(1), tex(2)];
        for (state, cell_h, rect) in [
            (live(0, 500), 20.0, r),
            (live(20, 500), 0.0, r),
            (live(20, 500), f32::NAN, r),
            (live(20, 500), 20.0, egui::Rect::from_min_size(r.min, egui::vec2(300.0, 0.0))),
        ] {
            let quads = band_quads(&[505], &textures, state, rect, cell_h);
            assert_eq!(quads.len(), 1, "state {state:?} cell_h {cell_h}");
            assert_eq!(quads[0].texture, tex(2), "the live look");
            assert!(quads[0].uv.top().is_finite() && quads[0].uv.bottom().is_finite());
        }
    }

    /// Scrolling moves the window, not the text. The same boundary paints further down the
    /// screen as the user scrolls back into history, and the older look grows to fill the
    /// rows above it — the visible half of `scroll_anchor`'s coordinate choice.
    #[test]
    fn scrolling_back_moves_the_band_edge_down_the_screen() {
        let r = pane();
        let at_live = band_quads(&[505], &[tex(1), tex(2)], live(20, 500), r, 20.0);
        assert_eq!(at_live[0].rect.height(), 100.0);

        let scrolled = ViewState { display_offset: 3, ..live(20, 500) };
        let quads = band_quads(&[505], &[tex(1), tex(2)], scrolled, r, 20.0);
        assert_eq!(quads[0].rect.height(), 160.0, "three more rows of the older look");
        assert_tiles(&quads, r);
    }

    /// 30 lines onto a 10-row grid, then linger so the pipe is not gone before we pump.
    /// Platform-split for `term.rs`'s reason: the *shells* differ, not just the paths.
    #[cfg(not(windows))]
    fn print_thirty_lines() -> Vec<String> {
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "i=0; while [ $i -lt 30 ]; do echo line$i; i=$((i+1)); done; sleep 0.3".into(),
        ]
    }
    #[cfg(windows)]
    fn print_thirty_lines() -> Vec<String> {
        vec![
            "cmd.exe".into(),
            "/C".into(),
            "(for /L %i in (1,1,30) do @echo line%i) & ping -n 2 127.0.0.1 >nul".into(),
        ]
    }

    /// **The counter's one update site, against a real PTY.**
    ///
    /// Two claims, and the first is the one a refactor breaks silently: [`PaneAnchor::pump`]
    /// must actually advance the parser — a console whose only pump call went through an
    /// anchor that forgot to call it would show a dead terminal, and every arithmetic test in
    /// this file would still pass. The second is `advance_dropped`'s contract from the caller
    /// side: with 30 lines against a 10 000-line buffer, nothing has been evicted, so a
    /// counter that reports anything but zero is inventing history.
    #[test]
    fn the_anchor_pumps_the_session_and_invents_no_evictions() {
        let mut session = TermSession::spawn(40, 10, Some(print_thirty_lines()), None)
            .expect("spawn a shell on a pty");
        let mut anchor = PaneAnchor::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            anchor.pump(&mut session);
            if session.term.history_size() > 0 {
                break;
            }
        }

        let state = anchor.view(&session);
        assert!(state.history > 0, "the anchor's pump never advanced the parser");
        assert_eq!(anchor.dropped(), 0, "30 lines cannot have overflowed a 10 000-line buffer");
        assert_eq!(state.dropped, 0);
        assert_eq!(state.rows, 10, "rows come from the session, not the grid's own idea");
        assert!(!state.alt_screen);
        assert_eq!(state.screen_top(), state.history as u64, "dropped 0 ⇒ screen_top IS history");
        assert_eq!(state.display_offset, 0, "at the live edge");

        // A look change dispatched now opens just below the cursor, inside the live screen.
        let at = anchor.boundary_now(&session);
        assert!(
            at > state.screen_top() && at <= state.live_edge(),
            "boundary {at} outside [{}, {}]",
            state.screen_top(),
            state.live_edge()
        );
    }

    // -------------------------------------------------------------------------
    // Reserved rows (Console Spike Tier 5)
    // -------------------------------------------------------------------------

    /// A child that produces nothing and stays alive, so the grid is ours alone while the
    /// block is fed. Platform-split for `term.rs`'s reason: the *shells* differ.
    #[cfg(not(windows))]
    fn silent_linger() -> Vec<String> {
        vec!["/bin/sh".into(), "-c".into(), "sleep 1".into()]
    }
    #[cfg(windows)]
    fn silent_linger() -> Vec<String> {
        vec!["cmd.exe".into(), "/C".into(), "ping -n 3 127.0.0.1 >nul".into()]
    }

    /// The row's glyphs, straight off the grid.
    fn row_text(session: &TermSession, line: i32) -> String {
        let cols = session.size().0;
        (0..cols)
            .map(|c| {
                let p = alacritty_terminal::index::Point::new(
                    alacritty_terminal::index::Line(line),
                    alacritty_terminal::index::Column(c as usize),
                );
                session.term.grid()[p].c
            })
            .collect()
    }

    /// **The rows genuinely exist, and the cursor lands below them.**
    ///
    /// This is the increment, as an assertion: after a block of `n`, the reserved rows are
    /// blank and the cursor sits one row further down, so the child's next line of output
    /// flows *below* the hole instead of over it. The absolute identity is checked with it —
    /// `cursor_abs == at + rows` is the arithmetic half of "one row below the block", and it
    /// is what a painter will later use to place a rect.
    ///
    /// Deliberately **no pump** anywhere in here: `feed_local` advances the parser against
    /// our bytes only, so the child's own output stays in the channel and the whole test is
    /// deterministic — which is also the claim being made about the mechanism.
    #[test]
    fn a_block_opens_blank_rows_and_leaves_the_cursor_below_them() {
        let mut session =
            TermSession::spawn(40, 10, Some(silent_linger()), None).expect("spawn a pty");
        let mut anchor = PaneAnchor::new();

        let before = anchor.view(&session);
        let cursor_before = session.term.grid().cursor.point.line.0;
        let at = anchor.feed_local(&mut session, &term::block_bytes(5));
        assert_eq!(
            at,
            before.screen_top() + (cursor_before as u64) + 1,
            "the block opens on the line just below the cursor"
        );

        let after = anchor.view(&session);
        let cursor_after = session.term.grid().cursor.point.line.0;
        let cursor_abs = after.screen_top() + cursor_after as u64;
        assert_eq!(cursor_abs, at + 5, "the cursor rests one row below a five-row block");
        assert_eq!(anchor.dropped(), 0, "six lines cannot overflow a 10 000-line buffer");

        // Every reserved row, blank — including the one the cursor is on.
        for row in at..=cursor_abs {
            let line = (row - after.screen_top()) as i32;
            assert_eq!(
                row_text(&session, line).trim(),
                "",
                "absolute line {row} (grid line {line}) is not blank"
            );
        }
    }

    /// The same identity when the block is **taller than the screen**, so the feed scrolls
    /// and `screen_top` moves under it. This is the case an accumulated coordinate would get
    /// wrong: 16 linefeeds against a 10-row grid push 7 lines into history, and the block's
    /// absolute index must not move with them.
    #[test]
    fn a_block_taller_than_the_screen_keeps_its_absolute_index() {
        let mut session =
            TermSession::spawn(40, 10, Some(silent_linger()), None).expect("spawn a pty");
        let mut anchor = PaneAnchor::new();

        let before = anchor.view(&session);
        assert_eq!(before.history, 0, "a fresh grid has no scrollback");
        let at = anchor.feed_local(&mut session, &term::block_bytes(15));

        let after = anchor.view(&session);
        assert!(after.history > 0, "a block that tall must have scrolled");
        assert_eq!(
            after.screen_top() + session.term.grid().cursor.point.line.0 as u64,
            at + 15,
            "scrolling moved the window, not the block"
        );
        assert_eq!(anchor.dropped(), 0);
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
