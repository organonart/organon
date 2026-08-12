//! What a claimed rectangle **shows** — and, for the panel kind, a **live egui control
//! panel drawn into the rows the writer left for it** (Console Spike Tier 5).
//!
//! [`block_anchor`](crate::block_anchor) answers *where* a claimed run of lines is this
//! frame; this module answers *what goes in it*. The two are deliberately separate, and the
//! separation is the shape of the whole tier: a claim, the anchor arithmetic and the per-pane
//! ledger are **common to every kind**, and the kind selects the last step only — the paint.
//! Add a kind and nothing before the paint changes.
//!
//! # The two kinds, and why they look so unequal
//!
//! [`PatchContent::Scene`] is a picture: the rendered substrate, sampled through the claimed
//! rows by [`crate::term_view::block_quads`]. [`PatchContent::Panel`] is not a picture of a
//! control panel — it is a control panel: a child [`egui::Ui`] at the block's rect, with real
//! sliders and real buttons, in the same egui pass and the same wgpu draw that already paints
//! every glyph.
//!
//! The obvious shape for the second one would have been another texture: render the panel
//! into an offscreen target, register it, sample it through the rows exactly as the scene is.
//! That is strictly *more* work and buys nothing. The console's whole frame is already one
//! egui pass; a block is a rect inside it, so a child `Ui` clipped to that rect is the entire
//! mechanism. No `TextureId`, no UV policy, no second `World`, no readback — and the controls
//! are alive rather than a photograph of controls, which is the claim the spike is making.
//!
//! # The three things that make a panel work
//!
//! 1. **Placement is [`block_anchor`](crate::block_anchor)'s, converted and nothing else.**
//!    [`placements`] turns each [`BlockBand`](crate::block_anchor::BlockBand) into two rects:
//!    `band`, the visible slice, and `full`, where the *whole* block would sit if none of it
//!    were scrolled off. Content is laid out into `full` and clipped to `band`, so a
//!    half-scrolled panel is **cut off** — the title slides up out of view — rather than
//!    squashed into the surviving rows. That is what `BlockBand::block_row` is for, and this
//!    is the caller that reads it.
//! 2. **The clip is the interaction boundary too.** egui intersects a widget's rect with the
//!    `Ui`'s clip rect to decide what the pointer can reach, so a slider scrolled off the top
//!    of its block stops responding at exactly the row it stops being visible at. One rect,
//!    both meanings.
//! 3. **A panel claims the pointer; a scene does not.** [`pointer_inside`] is what the
//!    terminal asks before handing the wheel to the child process: the console scrolls the
//!    transcript from anywhere in the window, so without this a slider drag would also scroll
//!    the block out from under the cursor. It is fed [`panel_placements`] rather than
//!    [`placements`] for the same reason — a scene patch is something to look at, and the
//!    wheel over it must keep scrolling the page, exactly as the wheel over a paragraph does.
//!    Mouse only: the keyboard stays the terminal's, deliberately.
//!
//! # What a panel is made of, and who decides
//!
//! [`BlockPanel`] carries its own title, its own slider values and its own **button labels**;
//! this module never learns what a button means. `organon-shell` cannot see
//! `substrate_materials`, and should not: the caller (`shell_main.rs`) fills the labels from
//! its own table and receives a [`BlockAction`] naming the one that was pressed, which it
//! feeds to the same `apply_console` a typed `organon console background <name>` reaches.
//! Clicking `metal` inside the scrollback and typing the command are the same code path from
//! that call onwards — which is the point of wiring it rather than simulating it.

use crate::block_anchor::{visible_blocks, Block};
use crate::scroll_anchor::ViewState;
use crate::term_view::DEFAULT_FG;

/// One claimed rectangle: the lines it is pinned to, and what the console draws in them.
///
/// The [`Block`] half never changes (two integers, fixed when the rectangle was claimed) and
/// is what every frame's arithmetic runs on; `content` is the only thing that distinguishes
/// one kind from another, and it is read for the first time at paint time.
#[derive(Clone, Debug)]
pub struct Patch {
    /// The lines this patch is pinned to. Never mutated — see [`crate::block_anchor`].
    pub block: Block,
    pub content: PatchContent,
}

/// The kinds a claim can name, as the console's side of them.
///
/// The **wire** side is `cli::PatchKind`, in the other crate: this is a paint target, that is
/// a word a writer types, and keeping them apart is what stops `organon-shell` from acquiring
/// an opinion about a command-line vocabulary it cannot see.
#[derive(Clone, Debug)]
pub enum PatchContent {
    /// The rendered scene, sampled through the rows. Carries no state of its own — the
    /// picture belongs to the console, which draws one and lets every scene patch sample it.
    Scene,
    /// A live control panel, with its own widget state.
    Panel(BlockPanel),
}

impl Patch {
    /// A patch that shows the rendered scene.
    pub fn scene(block: Block) -> Self {
        Self { block, content: PatchContent::Scene }
    }

    /// A patch that shows a live control panel.
    pub fn panel(block: Block, panel: BlockPanel) -> Self {
        Self { block, content: PatchContent::Panel(panel) }
    }

    /// Does this patch want the rendered picture? Read by the console to decide whether a
    /// scene has to be drawn at all this frame.
    pub fn is_scene(&self) -> bool {
        matches!(self.content, PatchContent::Scene)
    }
}

/// Every patch's lines, in order — the `&[Block]` the arithmetic in
/// [`crate::block_anchor`] takes.
///
/// **Index-preserving, and that is the point**: the ledger is one list across both kinds, so
/// a band's `block` index means the same thing to the scene painter and to the panel painter.
/// Filtering the list before the arithmetic instead of after would give each kind its own
/// index space and lose the z-order between them.
pub fn blocks(patches: &[Patch]) -> Vec<Block> {
    patches.iter().map(|p| p.block).collect()
}

/// One panel's state: everything about it that survives a frame.
///
/// Owned by the caller (per pane, beside the anchor) so that a slider dragged this frame is
/// still where it was dropped on the next one.
#[derive(Clone, Debug)]
pub struct BlockPanel {
    /// The line naming the patch, so it is obviously ours and not the shell's output.
    pub title: String,
    /// `(label, value)` in `0.0..=1.0`. They drive nothing yet **and that is stated rather
    /// than hidden**: what they demonstrate is that a drag inside the scrollback is a real
    /// drag, tracked across frames, not a repaint of a static image.
    pub sliders: Vec<(String, f32)>,
    /// Button labels, in row order. Their meaning is the caller's — see the module doc.
    pub buttons: Vec<String>,
}

/// The default slider set. Three, labelled, mid-range — enough that a drag is unmistakable
/// and the panel reads as an instrument rather than a decal.
pub const DEFAULT_SLIDERS: [(&str, f32); 3] =
    [("depth", 0.35), ("bloom", 0.60), ("drift", 0.20)];

impl BlockPanel {
    pub fn new(title: impl Into<String>, buttons: Vec<String>) -> Self {
        Self {
            title: title.into(),
            sliders: DEFAULT_SLIDERS.iter().map(|(l, v)| ((*l).to_string(), *v)).collect(),
            buttons,
        }
    }
}

/// A button was pressed in a panel: which patch, and which label.
///
/// A label rather than an index because the caller supplied the labels, and an index would
/// make the two lists something that can silently drift apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockAction {
    pub panel: usize,
    pub button: String,
}

/// Where one patch goes this frame: the slice showing, and the whole block it is a slice of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Index into the slice handed to [`placements`] — i.e. into the pane's patch ledger.
    pub panel: usize,
    /// The visible rows, in points. **This is the clip rect** and the pointer-claim rect.
    pub band: egui::Rect,
    /// Where the block's rows *would* be if none were scrolled off — `band` with the clipped
    /// part put back. Content is laid out into this, so a half-scrolled panel slides under
    /// the viewport edge instead of compressing. Equal to `band` whenever the whole block is
    /// on screen, which is the ordinary case.
    pub full: egui::Rect,
}

/// Every visible block's rects, in [`visible_blocks`]' order — which is the caller's z-order.
///
/// Rows to points and nothing else: the arithmetic is [`crate::block_anchor`]'s, swept
/// exhaustively there. A degenerate cell height or a collapsed rect yields nothing, because a
/// block that cannot be placed is the same nothing as a block that is off screen.
///
/// **Kind-blind on purpose.** It takes bare [`Block`]s so that it stays the one conversion
/// from rows to points whatever is going to be drawn in them; [`panel_placements`] is the
/// kind-aware wrapper, and it is a filter over this rather than a second copy of it.
pub fn placements(
    blocks: &[Block],
    state: ViewState,
    rect: egui::Rect,
    cell_h: f32,
) -> Vec<Placement> {
    if !(cell_h > 0.0) || rect.height() <= 0.0 {
        return Vec::new();
    }
    visible_blocks(blocks, state)
        .into_iter()
        .map(|band| {
            let top = (rect.top() + f32::from(band.first_vrow) * cell_h).min(rect.bottom());
            // `+ 1.0` in f32, not `+ 1` in u16: `last_vrow` is the *inclusive* last row and
            // the rect's bottom edge is the row after it.
            let bottom = (rect.top() + (f32::from(band.last_vrow) + 1.0) * cell_h)
                .min(rect.bottom())
                .max(top);
            let band_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), top),
                egui::pos2(rect.right(), bottom),
            );
            // The whole block, including the rows clipped away above and below: back up by
            // the block's own row offset, then take its full height.
            let full_top = top - f32::from(band.block_row) * cell_h;
            let full = egui::Rect::from_min_max(
                egui::pos2(rect.left(), full_top),
                egui::pos2(
                    rect.right(),
                    full_top + f32::from(blocks[band.block].rows) * cell_h,
                ),
            );
            Placement { panel: band.block, band: band_rect, full }
        })
        .collect()
}

/// The visible **panels** among a pane's patches — [`placements`] over the whole ledger, with
/// the other kinds dropped.
///
/// The whole ledger goes into the arithmetic and the filter happens after it, so `panel`
/// still indexes the caller's own list. Two callers, one answer: the wheel test and the draw
/// loop must agree exactly about which rectangles are panels, or the pointer would be claimed
/// by a rectangle nothing interactive is drawn in.
pub fn panel_placements(
    patches: &[Patch],
    state: ViewState,
    rect: egui::Rect,
    cell_h: f32,
) -> Vec<Placement> {
    let mut placed = placements(&blocks(patches), state, rect, cell_h);
    placed.retain(|pl| {
        patches.get(pl.panel).is_some_and(|p| matches!(p.content, PatchContent::Panel(_)))
    });
    placed
}

/// Is the pointer inside any placed panel?
///
/// The terminal asks this before giving the wheel to the child process. `None` (no pointer in
/// the window at all) is `false`, so a console nobody is pointing at scrolls exactly as it
/// always did.
pub fn pointer_inside(placements: &[Placement], pointer: Option<egui::Pos2>) -> bool {
    let Some(p) = pointer else { return false };
    placements.iter().any(|pl| pl.band.contains(p))
}

/// The panel's own surface: dark enough to read widgets against, translucent enough that the
/// scene behind the glass still shows. A rounded rect and a phosphor hairline, so the block
/// announces itself as a thing rather than a smudge.
const PANEL_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(0x0b, 0x12, 0x0e, 0xe6);
const PANEL_EDGE: egui::Color32 = egui::Color32::from_rgb(0x3e, 0x7a, 0x52);
/// Inset from the block's rows to the content, in points. One padding, used for both axes.
const PAD: f32 = 8.0;

/// Draw every visible panel and report the buttons that were pressed.
///
/// # Where this is called from, and why the position is the whole visual claim
///
/// [`crate::term_view::draw`] calls it **between the scrim and the glyph loop**, in the same
/// place and for the same reasons the scene quads are painted: above the backdrop and above
/// the legibility scrim, so a patch is the one place the engine is not dimmed; below every
/// glyph and the cursor, so nothing a panel draws can cover a character *by construction*
/// rather than because claimed rows happen to be blank. Same layer, so egui's own call order
/// is what enforces it — no z-index, nothing to keep in sync.
///
/// Widgets registered here are hit-tested after the panel's own background claim, so the
/// sliders and buttons win the pointer over the surface they sit on, which is the ordinary
/// meaning of drawing them later.
///
/// Patches of another kind are skipped, not merely drawn empty: a scene has already been
/// painted by then, and a panel surface over it would be a lid on the picture.
pub fn draw(
    ui: &mut egui::Ui,
    patches: &mut [Patch],
    state: ViewState,
    rect: egui::Rect,
    cell_h: f32,
) -> Vec<BlockAction> {
    // Placed first, in full, so the immutable borrow of the ledger ends before the loop takes
    // a mutable one — and so the geometry every panel is drawn at came from a single pass.
    let placed = panel_placements(patches, state, rect, cell_h);
    let mut actions = Vec::new();

    for pl in placed {
        let Some(Patch { content: PatchContent::Panel(panel), .. }) = patches.get_mut(pl.panel)
        else {
            continue;
        };

        // The surface, clipped to the visible rows so the rounding does not draw a lid on a
        // panel whose top is off screen.
        let painter = ui.painter_at(pl.band);
        painter.rect_filled(pl.full, 6.0, PANEL_FILL);
        painter.rect_stroke(
            pl.full,
            6.0,
            egui::Stroke::new(1.0_f32, PANEL_EDGE),
            egui::StrokeKind::Inside,
        );

        // The pointer claim, registered BEFORE the widgets so the widgets sit on top of it.
        // Without it a drag that starts on the panel's background would fall through to
        // whatever the console does with a drag, and the wheel test in `term_view::draw`
        // would be the only thing standing between a slider and the scrollback.
        let claim_id = ui.id().with(("organon-block-claim", pl.panel));
        let _ = ui.interact(pl.band, claim_id, egui::Sense::click_and_drag());

        let content = pl.full.shrink(PAD);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("organon-block", pl.panel))
                .max_rect(content)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        // Both meanings at once (module doc): what is painted, and what the pointer reaches.
        child.set_clip_rect(pl.band.intersect(ui.clip_rect()));
        child.spacing_mut().slider_width = 150.0;
        child.spacing_mut().item_spacing.y = 4.0;
        child.visuals_mut().override_text_color = Some(DEFAULT_FG);

        child.label(
            egui::RichText::new(&panel.title)
                .monospace()
                .strong()
                .color(egui::Color32::from_rgb(0x8f, 0xe0, 0xa8)),
        );

        child.horizontal_wrapped(|ui| {
            for label in &panel.buttons {
                if ui.button(egui::RichText::new(label).monospace()).clicked() {
                    actions.push(BlockAction { panel: pl.panel, button: label.clone() });
                }
            }
        });

        for (label, value) in panel.sliders.iter_mut() {
            child.add(egui::Slider::new(value, 0.0..=1.0).text(label.as_str()));
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 400-point-tall pane of 20-point rows — `term_view`'s own band-test shape.
    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 30.0), egui::vec2(300.0, 400.0))
    }

    fn live(rows: u16, history: usize) -> ViewState {
        ViewState { rows, display_offset: 0, history, dropped: 0, alt_screen: false }
    }

    fn blk(first_abs: u64, rows: u16) -> Block {
        Block { first_abs, rows }
    }

    /// A panel lands on cell boundaries, and while it is wholly on screen `full == band` —
    /// the ordinary case, in which the "slide under the edge" machinery must be inert.
    #[test]
    fn a_panel_covers_its_rows_and_is_unclipped_when_it_fits() {
        let r = pane();
        let out = placements(&[blk(505, 4)], live(20, 500), r, 20.0);
        assert_eq!(out.len(), 1);
        let p = out[0];
        assert_eq!((p.band.left(), p.band.right()), (r.left(), r.right()), "full width");
        assert_eq!(p.band.top(), r.top() + 100.0, "row 5 of 20-point rows");
        assert_eq!(p.band.bottom(), r.top() + 180.0, "four rows, bottom edge exclusive");
        assert_eq!(p.full, p.band, "nothing scrolled off, so nothing to put back");
    }

    /// Off screen is nothing; half off is **cut**, not squashed — `full` keeps the block's
    /// whole height and reaches above the viewport by exactly the rows that are gone, which is
    /// what makes the title slide away instead of the layout compressing.
    #[test]
    fn a_clipped_panel_keeps_its_full_height_above_the_edge() {
        let r = pane();
        let state = live(20, 500);
        assert!(placements(&[blk(400, 4)], state, r, 20.0).is_empty(), "above");
        assert!(placements(&[blk(600, 4)], state, r, 20.0).is_empty(), "below");
        assert!(placements(&[], state, r, 20.0).is_empty(), "no blocks at all");

        // Rows 496..=503: the top four are above the viewport.
        let p = placements(&[blk(496, 8)], state, r, 20.0)[0];
        assert_eq!(p.band.top(), r.top(), "clipped at the pane's edge");
        assert_eq!(p.band.bottom(), r.top() + 80.0);
        assert_eq!(p.full.top(), r.top() - 80.0, "the four lost rows, put back");
        assert_eq!(p.full.height(), 160.0, "eight rows, whatever is showing");

        // Degenerate geometry, and the alt screen, are both simply nothing.
        for (cell_h, rect) in [
            (0.0, r),
            (-4.0, r),
            (f32::NAN, r),
            (20.0, egui::Rect::from_min_size(r.min, egui::vec2(300.0, 0.0))),
        ] {
            assert!(placements(&[blk(505, 4)], state, rect, cell_h).is_empty(), "{cell_h}");
        }
        let alt = ViewState { alt_screen: true, ..state };
        assert!(placements(&[blk(505, 4)], alt, r, 20.0).is_empty());
    }

    /// The pointer claim is the visible band, not the whole block: a slider that has scrolled
    /// off the top must not still be swallowing the wheel from where it used to be.
    #[test]
    fn only_the_visible_band_claims_the_pointer() {
        let r = pane();
        let out = placements(&[blk(496, 8)], live(20, 500), r, 20.0);
        assert!(pointer_inside(&out, Some(egui::pos2(100.0, r.top() + 40.0))), "inside");
        assert!(!pointer_inside(&out, Some(egui::pos2(100.0, r.top() + 200.0))), "below it");
        assert!(!pointer_inside(&out, None), "no pointer in the window at all");
        assert!(!pointer_inside(&[], Some(egui::pos2(100.0, r.top() + 40.0))), "no panels");
    }

    /// **The kind decides who gets the wheel, and the index space stays the ledger's.** A
    /// scene patch is something to look at, so the wheel over one must keep scrolling the
    /// transcript; a panel takes it. The filter runs *after* the arithmetic, so `panel` still
    /// indexes the pane's own list — the failure this pins is a panel drawn at a scene's rect,
    /// which is a wrong-rectangle bug and invisible until something is dragged.
    #[test]
    fn only_panels_are_placed_and_they_keep_their_ledger_index() {
        let r = pane();
        let state = live(20, 500);
        let patches = vec![
            Patch::scene(blk(502, 3)),
            Patch::panel(blk(506, 4), BlockPanel::new("t", vec!["metal".into()])),
            Patch::scene(blk(512, 2)),
        ];
        let all = placements(&blocks(&patches), state, r, 20.0);
        assert_eq!(all.len(), 3, "the arithmetic sees every kind");

        let panels = panel_placements(&patches, state, r, 20.0);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].panel, 1, "the index is into the ledger, not into the panels");
        assert_eq!(panels[0].band.top(), r.top() + 120.0, "row 6, the panel's own rows");

        // The wheel over a scene patch still belongs to the transcript.
        assert!(pointer_inside(&panels, Some(egui::pos2(100.0, r.top() + 130.0))), "the panel");
        assert!(!pointer_inside(&panels, Some(egui::pos2(100.0, r.top() + 50.0))), "a scene");
        assert!(
            panel_placements(&[Patch::scene(blk(505, 4))], state, r, 20.0).is_empty(),
            "a ledger of scenes claims nothing"
        );
    }
}
