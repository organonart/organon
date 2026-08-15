//! Tune the palette while looking at it, instead of editing `theme.rs` and rebuilding.
//!
//! The colours in [`crate::theme`] were each chosen against a described intent — "fainter than
//! the track and still not the band it sits on" — and every one of them was then written as a
//! hex literal and compiled. That is a fine way to *state* a palette and a hopeless way to
//! *judge* one: the only way to find out whether `light`'s whitest white is too bright is to
//! look at it, and until now the loop for that was edit, rebuild, relaunch, look — about seven
//! minutes for a change that takes a second to evaluate. James, 2026-08-14: *"a little editor
//! where it shows the theme and the theme colors, and each one has a little HSV editor on it."*
//!
//! # Where it draws, and why there
//!
//! In the band above the composer — the region §1.9 built for the command panel — reusing that
//! module's `plate`, its padding and its row metric rather than inventing a second geometry for
//! the same strip. It is reached by `/theme edit` (and `/theme adjust`, which is the same
//! thing: James named both and neither is more correct).
//!
//! 🚨 **It takes the band from both the receipt and the candidate list while it is open**, and
//! that is a decision rather than a layering accident. Those two are transient — they answer a
//! line that is being typed or has just been sent — while the editor is a surface a hand is
//! *working in*, and a surface that vanished because a keystroke landed in the composer would be
//! unusable. `command_panel` therefore checks for the editor first and returns.
//!
//! # 🚨 The HSV is the truth, not the RGB
//!
//! **RGB → HSV → RGB does not round-trip**, and the failure is not a rounding error — it is a
//! loss of information that no amount of precision recovers. A grey has no hue at all: drag
//! saturation to nought and the hue that was there is gone from the bytes, so an editor that
//! re-derived HSV from the colour every frame would show hue 0 (red) the instant a colour went
//! neutral, and dragging saturation back up would return red rather than the blue it had been.
//! Value behaves the same way at nought, where black erases both.
//!
//! So [`ThemeEditor::hsv`] holds the `Hsva` for every field a hand has touched, and **that map
//! is authoritative for those fields until the editor closes**. The palette's `Color32` is
//! derived from it, never the other way round. A field nobody has touched is not in the map and
//! is read straight off the palette, which is what keeps the state proportional to the editing
//! rather than to the sixty-eight colours.
//!
//! ⚠️ **This is also why the editor does not use `egui::color_picker::color_picker_color32`.**
//! That function solves the same problem with a cache in egui's context memory keyed by the
//! `Color32` — and this palette is full of fields that deliberately hold identical bytes
//! (`human_text`, `tab_active`, `tab_menu_installed` and `term_fg` are all `#c8e6c8`, and the
//! module doc for `theme` explains at length why they are not merged). Keyed by value, those
//! four share one cache entry, so the hue held for one is the hue held for all four. Keyed by
//! *field*, as here, they do not.
//!
//! # What persistence means, stated out loud
//!
//! Three separate things, deliberately not gated on each other:
//!
//! 1. **Every drag repaints immediately.** The edit leaves on
//!    [`crate::conversation_view::ConversationOutput`] and `console_main` — which owns the one
//!    `Theme` — assigns it and re-derives egui's chrome. Nothing is written.
//! 2. **`/theme save` stores it**, as the *difference* from the compiled palette
//!    ([`crate::theme::Theme::diff`]), under the palette's own name.
//! 3. **`/theme revert` drops the overrides** and returns to the palette as this build compiles
//!    it.
//!
//! 🚨 **Unsaved is visible, on the head row, always.** An editor whose changes evaporate at exit
//! without having said so is worse than no editor: the tuning felt done. The head row carries
//! `unsaved` and a count whenever the working palette differs from the stored one, and the word
//! is absent only when they agree.

use std::collections::BTreeMap;

use egui::ecolor::Hsva;
use egui::Color32;

use crate::theme::{Theme, ANSI16_NAMES};

/// The words `/theme` accepts that are **not** palette names — they open this editor instead.
///
/// 🚨 **Both, and neither is the alias.** James named `edit` and `adjust` in one breath and did
/// not choose between them; a surface whose whole purpose is that you never have to remember
/// which word it wanted should not then insist on one. They are values of `console.theme`'s
/// existing argument rather than a verb of their own, which is what makes the command panel
/// complete them for free from the same table it completes palette names from — see §1.9.
pub const EDIT_WORDS: [&str; 2] = ["edit", "adjust"];

/// Is this word a request to open the editor rather than to paint a named palette?
pub fn is_edit_word(word: &str) -> bool {
    EDIT_WORDS.contains(&word)
}

/// How many field rows the editor shows at once.
///
/// The same eight the candidate list uses, for the same measured reason: a vertical
/// `ScrollArea` dropped into this bottom-up column takes the whole pane. A group longer than
/// this pages with the arrow keys rather than scrolling — the highlight moves, the window
/// follows it, and no row is unreachable.
pub const VISIBLE_ROWS: usize = 8;

/// What one key press means to the editor while it is open.
///
/// A table of plain values for [`crate::conversation_view::palette_key`]'s reason: egui's
/// modifier matching is shift-permissive, and a table is the only place that hazard is legible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    /// Move the highlight down the current group's fields, and up them. Wraps.
    NextField,
    PrevField,
    /// Move to the next ring of fields, and the previous one. Wraps.
    NextGroup,
    PrevGroup,
    /// Shut the editor. **Keeps the edits** — they are already painted, and throwing them away
    /// on the key people press to dismiss things would be the opposite of what that key means
    /// everywhere else.
    Close,
    /// Not the editor's business.
    Ignore,
}

/// 🚨 **Tab moves rings and Enter is untouched**, which is §1.9's rule unchanged.
///
/// The composer is still where a human talks to the agent, and the editor is drawn *above* it
/// without taking its focus — so Enter has to keep meaning "send", exactly as it does when no
/// editor is open. The editor claims Tab (rings), the arrows (fields and rings) and Escape
/// (close), and nothing else. In particular it claims **no printing key**, so the composer
/// still types normally with the editor open and a message can be sent without closing it.
///
/// ⚠️ `matches_exact` throughout, never `matches_logically` — the trap `composer_key` documents.
pub fn edit_key(key: egui::Key, mods: egui::Modifiers) -> EditKey {
    let bare = mods.matches_exact(egui::Modifiers::NONE);
    let shifted = mods.matches_exact(egui::Modifiers::SHIFT);
    match key {
        egui::Key::Tab if bare => EditKey::NextGroup,
        egui::Key::Tab if shifted => EditKey::PrevGroup,
        egui::Key::ArrowDown if bare => EditKey::NextField,
        egui::Key::ArrowUp if bare => EditKey::PrevField,
        egui::Key::ArrowRight if bare => EditKey::NextGroup,
        egui::Key::ArrowLeft if bare => EditKey::PrevGroup,
        egui::Key::Escape if bare => EditKey::Close,
        _ => EditKey::Ignore,
    }
}

/// A live editing session over one palette.
#[derive(Debug, Clone)]
pub struct ThemeEditor {
    /// The palette's canonical name — one of [`Theme::NAMES`]. What a saved override is filed
    /// under, so tuning `light` and then switching to `chocolate` does not apply one's edits to
    /// the other.
    name: String,
    /// The palette as edited. **Derived from [`ThemeEditor::hsv`] for every touched field**;
    /// this is what gets painted and what gets stored.
    working: Theme,
    /// The palette as the store currently holds it. What `unsaved` is measured against — not
    /// the compiled palette, or every launch with a saved override would open claiming to be
    /// unsaved.
    stored: Theme,
    /// 🚨 **The hue, saturation and value of every field a hand has touched — authoritative.**
    /// See the module doc: RGB cannot round-trip through HSV, so the palette's `Color32` is
    /// derived from this and never the reverse. Keyed by field name, because keying by colour
    /// is what egui's own picker does and this palette has four fields sharing one value.
    hsv: BTreeMap<&'static str, Hsva>,
    /// Which ring of [`Theme::editor_groups`] is showing.
    group: usize,
    /// Which field within it is highlighted.
    row: usize,
}

impl ThemeEditor {
    /// Open on a palette, optionally with one field already highlighted.
    ///
    /// `focus` is the field named by `/theme edit <field>`; an unknown name simply opens at the
    /// first ring rather than refusing, because the completion that produced it can only offer
    /// real ones and a hand-typed miss is better answered by showing the editor than by a
    /// message about it.
    pub fn open(theme: &Theme, name: &str, focus: Option<&str>) -> Self {
        let mut editor = Self {
            name: name.to_string(),
            working: theme.clone(),
            stored: theme.clone(),
            hsv: BTreeMap::new(),
            group: 0,
            row: 0,
        };
        if let Some(field) = focus {
            editor.focus(field);
        }
        editor
    }

    /// Put the highlight on a named field, if it is one. `false` if no ring holds it.
    pub fn focus(&mut self, field: &str) -> bool {
        for (g, (_, fields)) in Theme::editor_groups().iter().enumerate() {
            if let Some(r) = fields.iter().position(|f| *f == field) {
                self.group = g;
                self.row = r;
                return true;
            }
        }
        false
    }

    /// The palette being painted right now.
    pub fn working(&self) -> &Theme {
        &self.working
    }

    /// The palette's canonical name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many colours differ from what the store holds. Nought is the saved state, and is the
    /// only state in which the head row says nothing about saving.
    pub fn unsaved(&self) -> usize {
        self.working.diff(&self.stored).len()
    }

    /// Every colour that differs from the **compiled** palette — what a save writes down. Not
    /// the same question as [`ThemeEditor::unsaved`]: this one is "what is an override", that
    /// one is "what has not been written yet".
    pub fn overrides(&self) -> Vec<(&'static str, Color32)> {
        match Theme::by_name(&self.name) {
            Some(compiled) => self.working.diff(&compiled),
            // A palette this build does not have cannot be diffed against, so nothing is an
            // override. Unreachable while `name` comes from `Theme::resolve`, and a silent
            // empty answer is the right failure anyway: it stores nothing rather than storing
            // sixty-eight colours against a name nobody can resolve.
            None => Vec::new(),
        }
    }

    /// Mark the working palette as the stored one — called after the write actually succeeded,
    /// never before, so a failed save keeps saying `unsaved`.
    pub fn mark_stored(&mut self) {
        self.stored = self.working.clone();
    }

    /// Throw away every override and return to the palette as this build compiles it.
    pub fn revert(&mut self) {
        if let Some(compiled) = Theme::by_name(&self.name) {
            self.working = compiled;
            // The held HSV described the colours that are now gone; keeping it would make the
            // next drag jump back to a hue the palette no longer has.
            self.hsv.clear();
        }
    }

    /// The rings, with the current one's index — what the head row reports.
    pub fn rings(&self) -> (usize, usize) {
        (self.group, Theme::editor_groups().len())
    }

    /// The heading and full field list of the ring showing now.
    pub fn ring(&self) -> (&'static str, Vec<&'static str>) {
        let groups = Theme::editor_groups();
        let (name, fields) = groups[self.group.min(groups.len() - 1)].clone();
        (name, fields)
    }

    /// The fields actually drawn this frame, and how many of the ring are above and below them.
    ///
    /// The window follows the highlight rather than paging in blocks: moving down off the last
    /// visible row scrolls by one, which keeps the row a hand is working on adjacent to the one
    /// it just left. Paging would move it half a screen away.
    pub fn window(&self) -> (usize, Vec<&'static str>) {
        let (_, fields) = self.ring();
        if fields.len() <= VISIBLE_ROWS {
            return (0, fields);
        }
        let last = fields.len() - VISIBLE_ROWS;
        let first = self.row.saturating_sub(VISIBLE_ROWS - 1).min(last);
        (first, fields[first..first + VISIBLE_ROWS].to_vec())
    }

    /// Which field is highlighted.
    pub fn selected(&self) -> &'static str {
        let (_, fields) = self.ring();
        fields[self.row.min(fields.len() - 1)]
    }

    /// Act on one key. Returns `false` when the editor should close.
    pub fn key(&mut self, key: EditKey) -> bool {
        let groups = Theme::editor_groups();
        let len = self.ring().1.len();
        match key {
            EditKey::NextField => self.row = (self.row + 1) % len,
            EditKey::PrevField => self.row = (self.row + len - 1) % len,
            EditKey::NextGroup => {
                self.group = (self.group + 1) % groups.len();
                self.row = 0;
            }
            EditKey::PrevGroup => {
                self.group = (self.group + groups.len() - 1) % groups.len();
                self.row = 0;
            }
            EditKey::Close => return false,
            EditKey::Ignore => {}
        }
        true
    }

    /// The HSV a field is being edited through — held if it has been touched, derived from the
    /// palette if it has not.
    pub fn hsva(&self, field: &str) -> Hsva {
        self.hsv
            .get(field)
            .copied()
            .or_else(|| self.working.field(field).map(Hsva::from))
            .unwrap_or_else(|| Hsva::new(0.0, 0.0, 0.0, 1.0))
    }

    /// Write a field's HSV, and derive its colour from it.
    ///
    /// 🚨 **The held HSV is written whether or not the resulting `Color32` changed**, and that
    /// is the point of holding it: dragging hue across a fully-desaturated colour changes no
    /// byte at all, and an editor that only recorded byte changes would snap the hue back to
    /// where it started on the next frame. `false` for a name no field answers to.
    /// ⚠️ The name is interned to `'static` against [`Theme::SCALAR_FIELDS`] and
    /// [`ANSI16_NAMES`] — the two `&'static` tables — rather than by building a palette and
    /// asking it for its fields. Both answer the same question, and this one runs on **every
    /// frame of a live drag**: the palette version allocated a fresh `Theme` and a 68-entry
    /// `Vec` per tick to learn a fact that is a compile-time constant.
    pub fn set_hsva(&mut self, field: &str, hsva: Hsva) -> bool {
        let Some(name) = Theme::SCALAR_FIELDS
            .iter()
            .chain(ANSI16_NAMES.iter())
            .find(|n| **n == field)
            .copied()
        else {
            return false;
        };
        self.hsv.insert(name, hsva);
        self.working.set_field(name, Color32::from(hsva))
    }
}

/// What the editor asks the console to do with the palette it just changed.
///
/// The editor cannot do any of it itself: `conversation_view` is handed `&Theme`, and the one
/// owner is `console_main`'s `Console` — see [`crate::theme`]'s "one owner, no globals". So this
/// rides out on [`crate::conversation_view::ConversationOutput`] and is applied there.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeChange {
    /// The palette to paint from the next frame.
    pub theme: Theme,
    /// Its canonical name, so an override is filed under the palette it was tuned against.
    pub name: String,
    /// What should happen to `preferences.json`.
    pub store: StoreAction,
}

/// The three things a change can ask of the store. **Painting is not one of them** — every
/// change paints; this is only about what is written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreAction {
    /// Write nothing. What a drag does: the palette repaints and the head row says `unsaved`.
    Leave,
    /// Write this palette's overrides under its name.
    Save,
    /// Remove this palette's overrides entirely.
    Clear,
}

/// The head row's two buttons and the key line, in the order they are drawn.
const SAVE_LABEL: &str = "save";
const REVERT_LABEL: &str = "revert";
/// ⚠️ **ASCII only**, like every other string this band draws — `conversation_view`'s glyph
/// allowlist test walks this module's constants too. The obvious separator here is an en dash
/// and it is in none of egui's four bundled fonts.
const EDIT_KEYS: &str = "Tab ring - arrows field - Esc close";

impl ThemeEditor {
    /// How many text rows the band needs: the head, the fields on show, and the key line.
    ///
    /// Asked *before* anything is drawn, because the band is **reserved, not discovered** —
    /// `conversation_view::plate`'s measured rule: a child that places itself at
    /// `available_rect_before_wrap().min` in this bottom-up column takes the whole pane.
    pub fn band_rows(&self) -> usize {
        2 + self.window().1.len()
    }

    /// Draw the editor, and report what it wants done. `None` means nothing changed this frame.
    ///
    /// ⚠️ **Returns `Some` only on a real change**, never every frame. `console_main` re-issues
    /// `set_visuals` on every change it is handed, and egui holds `Visuals` on the context
    /// rather than reading it per frame — so answering `Some` unconditionally would rebuild and
    /// re-upload the whole chrome sixty times a second for a palette nobody was touching.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme) -> Option<ThemeChange> {
        let mut action: Option<StoreAction> = None;
        let unsaved = self.unsaved();
        let (index, rings) = self.rings();
        let (ring, _) = self.ring();

        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("theme edit - {}", self.name))
                        .monospace()
                        .color(theme.panel_title),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{ring} {}/{rings}", index + 1))
                        .monospace()
                        .color(theme.dim),
                )
                .truncate(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 🚨 The unsaved count is drawn from the palette's *alert* colour rather than
                // its error colour, and it is on the right where the eye finishes the row. A
                // tuning session that evaporates at exit without having said so is the failure
                // this word exists to prevent; it is not an error, so it must not read as one.
                if unsaved > 0 {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{unsaved} unsaved"))
                                .monospace()
                                .color(theme.mode_alert),
                        )
                        .truncate(),
                    );
                }
                if ui.button(REVERT_LABEL).on_hover_text(REVERT_HOVER).clicked() {
                    self.revert();
                    action = Some(StoreAction::Clear);
                }
                if ui.button(SAVE_LABEL).on_hover_text(SAVE_HOVER).clicked() {
                    action = Some(StoreAction::Save);
                }
            });
        });

        let (_, window) = self.window();
        let selected = self.selected();
        for field in window {
            let here = field == selected;
            let mut hsva = self.hsva(field);
            let before = hsva;
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if here { HERE } else { THERE })
                            .monospace()
                            .color(if here { theme.ok } else { theme.dim }),
                    )
                    .truncate(),
                );
                // The swatch is the only thing in the row that is not a word, and it is what
                // makes the row scannable: a name and three numbers do not tell you what a
                // colour looks like.
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(SWATCH, ui.text_style_height(&egui::TextStyle::Monospace)),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rect, 2.0, Color32::from(hsva));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{field:<FIELD_WIDTH$}"))
                            .monospace()
                            .color(if here { theme.human_text } else { theme.prose }),
                    )
                    .truncate(),
                );

                // 🚨 Three separate drags over the held HSV, never over the colour. Degrees and
                // percent rather than egui's 0..1 because a hand asked to "turn the brightness
                // down a bit" thinks in percent; the scaling is undone on the way back in.
                //
                // 🚨 **The comparison is on the scaled numbers, not on the `Hsva`, and that is
                // load-bearing rather than tidy.** `h * 360.0 / 360.0` is not `h` in binary
                // floating point, so writing the scaled values back unconditionally makes every
                // field differ from itself on the very first frame — which would report a
                // change nobody made, sixty times a second, and mark a freshly opened palette
                // `unsaved`. `DragValue::range` clamping is the second way the same thing
                // happens. Comparing what the widget was *given* against what it *returned* is
                // the only form of this that can be quiet when nothing moved.
                let mut h = hsva.h * 360.0;
                let mut s = hsva.s * 100.0;
                let mut v = hsva.v * 100.0;
                let (was_h, was_s, was_v) = (h, s, v);
                ui.add(egui::DragValue::new(&mut h).speed(1.0).range(0.0..=360.0).prefix("H "));
                ui.add(egui::DragValue::new(&mut s).speed(0.5).range(0.0..=100.0).prefix("S "));
                ui.add(egui::DragValue::new(&mut v).speed(0.5).range(0.0..=100.0).prefix("V "));
                if h != was_h || s != was_s || v != was_v {
                    hsva.h = h / 360.0;
                    hsva.s = s / 100.0;
                    hsva.v = v / 100.0;
                }

                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("#{}", crate::theme::to_hex(Color32::from(hsva))))
                            .monospace()
                            .color(theme.dim),
                    )
                    .truncate(),
                );
            });
            if hsva != before {
                self.set_hsva(field, hsva);
                action.get_or_insert(StoreAction::Leave);
            }
        }

        ui.add(
            egui::Label::new(egui::RichText::new(EDIT_KEYS).monospace().color(theme.dim)).truncate(),
        );

        action.map(|store| ThemeChange {
            theme: self.working.clone(),
            name: self.name.clone(),
            store,
        })
    }
}

/// Every string this editor can put on screen, for `conversation_view`'s glyph allowlist test.
///
/// 🚨 **Enumerated rather than spot-checked, and the derived halves are in it.** The panel's own
/// guard learned this the expensive way: `✓` reached a third draw site because the test only
/// knew about the two it was written for. The strings that can go wrong here are not the
/// constants — those are one character each and visibly ASCII — but the **group headings**,
/// which are hand-written prose in `theme.rs`'s field macro, and the **field names**, which are
/// identifiers today and would not have to stay that way. Both are included.
pub(crate) fn drawn_strings() -> Vec<String> {
    let mut out = vec![
        HERE.to_string(),
        THERE.to_string(),
        EDIT_KEYS.to_string(),
        SAVE_LABEL.to_string(),
        REVERT_LABEL.to_string(),
        SAVE_HOVER.to_string(),
        REVERT_HOVER.to_string(),
        // The head row, assembled exactly as `ui` assembles it.
        format!("theme edit - {}", Theme::NAMES[0]),
        "3 unsaved".to_string(),
        // A drag's prefixes and a swatch's hex.
        "H ".to_string(),
        "S ".to_string(),
        "V ".to_string(),
        format!("#{}", crate::theme::to_hex(Theme::organon().prose)),
    ];
    for (index, (group, fields)) in Theme::editor_groups().into_iter().enumerate() {
        out.push(format!("{group} {}/{}", index + 1, Theme::GROUPS.len()));
        out.extend(fields.into_iter().map(|f| format!("{f:<FIELD_WIDTH$}")));
    }
    for name in Theme::NAMES {
        out.push(format!("theme edit - {name}"));
    }
    out
}

/// The mark on the highlighted row, and on every other one — `conversation_view`'s own, for the
/// same ASCII reason.
const HERE: &str = ">";
const THERE: &str = " ";
/// The swatch's width in points, and the column the field name is padded to so the three drags
/// line up down the list rather than stepping in and out with the name's length.
const SWATCH: f32 = 14.0;
const FIELD_WIDTH: usize = 24;
const SAVE_HOVER: &str = "write these colours to preferences.json, under this palette's name";
const REVERT_HOVER: &str = "forget every tuned colour and go back to the palette this build ships";

#[cfg(test)]
mod tests {
    use super::*;

    /// CONTRACT: both words open the editor, and neither is a palette.
    #[test]
    fn edit_and_adjust_both_open_the_editor_and_are_not_palettes() {
        for word in EDIT_WORDS {
            assert!(is_edit_word(word));
            assert!(
                Theme::by_name(word).is_none(),
                "`{word}` is both an edit word and a palette name, so `/theme {word}` is ambiguous"
            );
        }
        assert!(!is_edit_word("light"), "a palette name is not an edit word");
        assert!(!is_edit_word("Edit"), "refused, never case-folded - `Theme::resolve`'s rule");
    }

    /// 🚨 CONTRACT: **hue survives a trip through grey.** This is the whole reason the editor
    /// holds HSV rather than deriving it, and the failure it prevents is silent: drag
    /// saturation to nought, the bytes become a neutral that has no hue at all, and an editor
    /// re-deriving from those bytes would show red. Dragging saturation back up would then
    /// return red instead of the blue it was.
    #[test]
    fn a_hue_survives_being_dragged_through_grey() {
        let mut e = ThemeEditor::open(&Theme::light(), "light", None);
        let blue = Hsva::new(0.6, 0.8, 0.5, 1.0);
        assert!(e.set_hsva("term_bg", blue));

        // All the way to grey: the colour is now neutral and its bytes carry no hue.
        let grey = Hsva { s: 0.0, ..blue };
        assert!(e.set_hsva("term_bg", grey));
        let neutral = e.working().field("term_bg").unwrap();
        let [r, g, b, _] = neutral.to_array();
        assert!(r == g && g == b, "saturation nought should be neutral, got {neutral:?}");

        // The editor still knows what hue it was, which re-deriving from `neutral` could not.
        assert_eq!(e.hsva("term_bg").h, 0.6, "the hue was lost with the saturation");
        assert!(e.set_hsva("term_bg", Hsva { s: 0.8, ..e.hsva("term_bg") }));
        assert_eq!(e.working().field("term_bg").unwrap(), Color32::from(blue), "not the same blue");
    }

    /// 🚨 CONTRACT: **fields that share a colour do not share a hue.** Four fields hold
    /// `#c8e6c8` in `organon` on purpose; egui's own picker caches HSV by colour value, which
    /// would weld all four together. Keying by field is what keeps them apart.
    #[test]
    fn two_fields_holding_the_same_colour_are_edited_separately() {
        let t = Theme::organon();
        assert_eq!(t.human_text, t.tab_active, "the premise of this test moved");

        let mut e = ThemeEditor::open(&t, "organon", None);
        assert!(e.set_hsva("human_text", Hsva::new(0.1, 0.5, 0.5, 1.0)));
        assert_eq!(e.hsva("tab_active"), Hsva::from(t.tab_active), "the untouched one moved");
        assert_ne!(
            e.working().human_text,
            e.working().tab_active,
            "editing one field changed another that merely shared its bytes"
        );
    }

    /// CONTRACT: an untouched field reads straight off the palette, so opening the editor
    /// costs no state at all and `unsaved` is nought.
    #[test]
    fn opening_the_editor_changes_nothing_and_is_saved() {
        let t = Theme::chocolate();
        let e = ThemeEditor::open(&t, "chocolate", None);
        assert_eq!(e.unsaved(), 0);
        assert!(e.overrides().is_empty(), "a freshly opened editor overrides nothing");
        assert_eq!(e.working(), &t);
        assert_eq!(e.hsva("prose"), Hsva::from(t.prose));
    }

    /// CONTRACT: the head row's `unsaved` count is the number of colours that differ from the
    /// store, and `mark_stored` is what clears it — never the edit itself.
    #[test]
    fn unsaved_counts_what_the_store_has_not_been_told() {
        let mut e = ThemeEditor::open(&Theme::light(), "light", None);
        assert!(e.set_hsva("term_bg", Hsva::new(0.0, 0.0, 0.92, 1.0)));
        assert_eq!(e.unsaved(), 1);
        assert_eq!(e.overrides().len(), 1, "and it is an override of the compiled palette");
        assert_eq!(e.overrides()[0].0, "term_bg");

        assert!(e.set_hsva("prose", Hsva::new(0.0, 0.0, 0.2, 1.0)));
        assert_eq!(e.unsaved(), 2);

        e.mark_stored();
        assert_eq!(e.unsaved(), 0, "stored, so nothing is outstanding");
        assert_eq!(e.overrides().len(), 2, "…but both are still overrides of the compiled look");
    }

    /// CONTRACT: revert returns the compiled palette and forgets the held hues with it — a
    /// stale hue would make the next drag jump to a colour the palette no longer has.
    #[test]
    fn revert_returns_the_compiled_palette() {
        let mut e = ThemeEditor::open(&Theme::light(), "light", None);
        assert!(e.set_hsva("term_bg", Hsva::new(0.3, 0.9, 0.4, 1.0)));
        assert_ne!(e.working(), &Theme::light());
        e.revert();
        assert_eq!(e.working(), &Theme::light());
        assert!(e.overrides().is_empty());
        assert_eq!(e.hsva("term_bg"), Hsva::from(Theme::light().term_bg), "a hue outlived revert");
    }

    /// CONTRACT: every ring is reachable by Tab and wraps, and the field highlight wraps within
    /// a ring. A ring that could not be reached would hide its colours completely.
    #[test]
    fn tab_walks_every_ring_and_wraps() {
        let mut e = ThemeEditor::open(&Theme::organon(), "organon", None);
        let count = Theme::editor_groups().len();
        let mut seen = Vec::new();
        for _ in 0..count {
            seen.push(e.ring().0);
            assert!(e.key(EditKey::NextGroup));
        }
        assert_eq!(e.rings().0, 0, "Tab did not wrap back to the first ring");
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "Tab visits a ring twice and misses another");

        assert!(e.key(EditKey::PrevGroup));
        assert_eq!(e.rings().0, count - 1, "Shift+Tab did not wrap backwards");
    }

    /// 🚨 CONTRACT: **every colour is reachable by hand.** The window slides with the highlight
    /// rather than paging, and the timeline ring is longer than the eight rows on show — so
    /// this walks each ring end to end and demands the drawn window actually contain the
    /// selected field every step of the way. A colour that is in the palette, in a ring, and
    /// never drawn is one nobody can tune.
    #[test]
    fn every_colour_can_be_walked_to_and_is_drawn_when_it_is_selected() {
        let mut e = ThemeEditor::open(&Theme::organon(), "organon", None);
        let mut reached = Vec::new();
        for _ in 0..Theme::editor_groups().len() {
            let (_, fields) = e.ring();
            for _ in 0..fields.len() {
                let selected = e.selected();
                let (_, window) = e.window();
                assert!(
                    window.contains(&selected),
                    "`{selected}` is highlighted but not among the {} rows drawn: {window:?}",
                    window.len()
                );
                assert!(window.len() <= VISIBLE_ROWS, "{window:?} overflows the band");
                reached.push(selected);
                assert!(e.key(EditKey::NextField));
            }
            assert_eq!(e.selected(), fields[0], "the field highlight did not wrap");
            assert!(e.key(EditKey::NextGroup));
        }
        reached.sort_unstable();
        reached.dedup();
        assert_eq!(
            reached.len(),
            Theme::organon().fields().len(),
            "some colour cannot be reached with the arrow keys"
        );
    }

    /// CONTRACT: `/theme edit <field>` lands on that field, in its own ring.
    #[test]
    fn opening_on_a_named_field_lands_on_it() {
        let t = Theme::light();
        for field in ["human_text", "ansi_bright_white", "tab_menu_missing", "context_arc_high"] {
            let e = ThemeEditor::open(&t, "light", Some(field));
            assert_eq!(e.selected(), field, "did not land on `{field}`");
            let (_, window) = e.window();
            assert!(window.contains(&field), "`{field}` landed off the drawn window");
        }
        // An unknown name opens the editor anyway rather than refusing — see `open`.
        let e = ThemeEditor::open(&t, "light", Some("human_txt"));
        assert_eq!(e.rings().0, 0);
    }

    /// CONTRACT: Escape closes and every other key the editor claims does not.
    #[test]
    fn only_escape_closes() {
        let mut e = ThemeEditor::open(&Theme::organon(), "organon", None);
        for key in
            [EditKey::NextField, EditKey::PrevField, EditKey::NextGroup, EditKey::PrevGroup]
        {
            assert!(e.key(key), "{key:?} closed the editor");
        }
        assert!(!e.key(EditKey::Close));
    }

    /// 🚨 CONTRACT: **Enter and every printing key stay out of the editor's hands.** The
    /// composer is still live underneath it, so a key the editor claimed would be a key that
    /// silently stopped typing.
    #[test]
    fn the_editor_claims_no_key_the_composer_needs() {
        use egui::{Key, Modifiers};
        for key in [Key::Enter, Key::A, Key::Z, Key::Num0, Key::Space, Key::Backspace] {
            assert_eq!(
                edit_key(key, Modifiers::NONE),
                EditKey::Ignore,
                "{key:?} belongs to the composer"
            );
        }
        assert_eq!(edit_key(Key::Tab, Modifiers::NONE), EditKey::NextGroup);
        assert_eq!(edit_key(Key::Tab, Modifiers::SHIFT), EditKey::PrevGroup);
        assert_eq!(edit_key(Key::Escape, Modifiers::NONE), EditKey::Close);
        // Shift-permissiveness is the trap `composer_key` documents: a modified arrow is not
        // the editor's.
        assert_eq!(edit_key(Key::ArrowDown, Modifiers::COMMAND), EditKey::Ignore);
        assert_eq!(edit_key(Key::Escape, Modifiers::SHIFT), EditKey::Ignore);
    }

    /// CONTRACT: a field name nothing answers to is refused rather than silently creating one.
    #[test]
    fn an_unknown_field_cannot_be_written() {
        let mut e = ThemeEditor::open(&Theme::organon(), "organon", None);
        assert!(!e.set_hsva("human_txt", Hsva::new(0.5, 0.5, 0.5, 1.0)));
        assert!(!e.set_hsva("scrim_floor", Hsva::new(0.5, 0.5, 0.5, 1.0)));
        assert_eq!(e.working(), &Theme::organon());
        assert_eq!(e.unsaved(), 0);
    }
}
