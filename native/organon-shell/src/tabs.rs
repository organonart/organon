//! Strips, and the tab strip built from one (Shell #11 Tier 1, generalized in
//! Tier 3): the Superconductor model in terminal clothes.
//!
//! A **strip** is a single row of monospace chips pinned to one edge of the window —
//! the ONE permitted piece of chrome (PRD v3.1 FR-T11), styled like the founder's
//! Kitty setup: nothing rounded, nothing glossy. The console gets two of them from
//! this one widget, and the widget is careful to know about neither: the **tab strip**
//! runs along the **top**, one chip per harness session in its own PTY, and the
//! **discover strip** (Tier 3, wired up by the host — not here) runs along the
//! **bottom**, one chip per entry of a discovery document. [`strip`] takes labels in
//! and hands back the index the human tapped ([`StripHit`]); what an activation
//! *means* belongs to the caller.
//!
//! The tab-specific extras stay in [`tab_bar`]: the close affordance, and the **+**
//! control that drops the numbered harness list exactly like the Superconductor
//! screenshot — installed entries selectable, missing ones greyed with their install
//! URL on hover.
//!
//! The model ([`TabStrip`]) is pure and fully tested, as is the arithmetic deciding
//! how much of a payload fits ([`strip_fit`]) and what a chip says ([`tab_items`]);
//! the renderers only paint, and return an action for the host to apply — a strip
//! never mutates itself mid-frame, which is what keeps session lifetimes (spawn/kill)
//! in the host where they belong.

use crate::harness::HarnessSpec;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// The strip widget: labels in, an index out.
// ---------------------------------------------------------------------------

/// The hard cap on how many chips a strip draws. From `doc/console_discover_schema.md`
/// ("Console behaviour and failure modes"): more than ten entries **truncates to the
/// cap and never scrolls**. A strip is a row you take in at a glance; one that scrolls
/// is a menu wearing a strip's clothes.
pub const STRIP_ITEM_CAP: usize = 10;

const STRIP_FONT_SIZE: f32 = 12.0;
/// The strip's left margin, ahead of the first chip.
const STRIP_LEAD_SPACE: f32 = 6.0;
/// How far a strip's popup clears the control it hangs off.
const MENU_GAP: f32 = 8.0;
const CHIP_SELECTED: egui::Color32 = egui::Color32::from_rgb(0xc8, 0xe6, 0xc8);
const CHIP_IDLE: egui::Color32 = egui::Color32::from_rgb(0x60, 0x6c, 0x60);
const CHIP_DISABLED: egui::Color32 = egui::Color32::from_rgb(0x50, 0x5a, 0x50);
/// What a chip shows when its source offers no glyph of its own.
const FALLBACK_GLYPH: char = '❯';

/// Which edge of the window a strip is pinned to.
///
/// A popup belonging to a strip opens *away* from that edge, so the direction is a
/// property of where the strip sits and never a constant: the tab strip is at the top
/// and its menus hang down, a bottom strip's menus rise. (The **+** menu was anchored
/// upward for a strip that was imagined at the bottom and shipped at the top; this is
/// the type that stops that from being a guess.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripEdge {
    Top,
    Bottom,
}

impl StripEdge {
    /// Which corner of the popup is pinned to [`Self::menu_origin`].
    pub fn menu_pivot(self) -> egui::Align2 {
        match self {
            Self::Top => egui::Align2::LEFT_TOP,
            Self::Bottom => egui::Align2::LEFT_BOTTOM,
        }
    }

    /// Where a popup anchored to `control` hangs from: just clear of it, on the side
    /// away from the strip's own edge.
    pub fn menu_origin(self, control: egui::Rect) -> egui::Pos2 {
        let gap = egui::vec2(0.0, MENU_GAP);
        match self {
            Self::Top => control.left_bottom() + gap,
            Self::Bottom => control.left_top() - gap,
        }
    }
}

/// One chip: what it says, an optional single-character glyph in front of it, and
/// whether it can be tapped at all. A disabled chip still draws — greyed, present,
/// unclickable — because a strip that silently omits what it cannot offer teaches the
/// wrong shape of the world.
#[derive(Clone, Debug, PartialEq)]
pub struct StripItem {
    pub label: String,
    pub glyph: Option<char>,
    pub enabled: bool,
}

impl StripItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self { label: label.into(), glyph: None, enabled: true }
    }

    pub fn glyph(mut self, glyph: char) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The text the chip paints: `"<glyph> <label>"`, or the bare label without one.
    pub fn text(&self) -> String {
        match self.glyph {
            Some(g) => format!("{g} {}", self.label),
            None => self.label.clone(),
        }
    }
}

/// A glyph is *one* character by contract (`doc/console_discover_schema.md`: "A single
/// character"). A longer string is refused rather than narrowed to its first char —
/// half a glyph is a rendering bug wearing a valid value's clothes — and the caller
/// falls back to something it can vouch for.
pub fn sole_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// How much of a payload a strip draws, and how much it had to leave out. `dropped` is
/// what a `coverage` line reports; it is never a scrollbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StripFit {
    pub shown: usize,
    pub dropped: usize,
}

/// Apply [`STRIP_ITEM_CAP`]. Truncation keeps the payload's own order — the emitter
/// ranked it, and a strip that reordered would be lying about that ranking.
pub fn strip_fit(count: usize) -> StripFit {
    let shown = count.min(STRIP_ITEM_CAP);
    StripFit { shown, dropped: count - shown }
}

/// The prefix of `items` a strip may draw. Empty in, empty out: zero entries is a strip
/// with nothing to say, and the host hides the row entirely rather than drawing an
/// empty one.
pub fn visible(items: &[StripItem]) -> &[StripItem] {
    &items[..strip_fit(items.len()).shown]
}

/// What the human did to a strip this frame. Both fields are **indices into the slice
/// that was drawn**; what they mean is the caller's business — the widget knows nothing
/// about tabs, terminals or discovery documents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StripHit {
    /// Tapped with the primary button.
    pub activated: Option<usize>,
    /// Middle-clicked — "be rid of this one" wherever the caller has such a meaning.
    /// The tab strip closes the tab; a strip without that meaning ignores it.
    pub dismissed: Option<usize>,
}

/// Draw `items` as chips in the **current row** and report what was tapped.
///
/// Call this inside a horizontal layout when the row holds more than the chips (the tab
/// strip's **+** sits after them); [`strip_row`] is the one-call form for a caller that
/// owns the whole row. `selected` is the chip drawn lit, if any — `None` for a strip
/// where nothing is current, and an index past the end simply lights nothing.
///
/// No cap is applied here: a fixed set of chips is the caller's own count, while a
/// payload that arrived as data should come through [`visible`] first.
pub fn strip(ui: &mut egui::Ui, items: &[StripItem], selected: Option<usize>) -> StripHit {
    let mut hit = StripHit::default();
    let font = egui::FontId::monospace(STRIP_FONT_SIZE);
    ui.add_space(STRIP_LEAD_SPACE);
    for (i, item) in items.iter().enumerate() {
        let is_selected = selected == Some(i);
        let color = match (item.enabled, is_selected) {
            (false, _) => CHIP_DISABLED,
            (true, true) => CHIP_SELECTED,
            (true, false) => CHIP_IDLE,
        };
        let text = egui::RichText::new(item.text()).font(font.clone()).color(color);
        // The enabled arm is the tab strip's call verbatim — this is a refactor, and a
        // chip's geometry is not something to re-derive. Greying is a wrapper around it.
        let resp = if item.enabled {
            ui.selectable_label(is_selected, text)
        } else {
            ui.add_enabled_ui(false, |ui| ui.selectable_label(is_selected, text)).inner
        };
        if resp.clicked() {
            hit.activated = Some(i);
        }
        if resp.middle_clicked() {
            hit.dismissed = Some(i);
        }
    }
    hit
}

/// A strip that owns its whole row: vertically centred, and capped per the schema.
pub fn strip_row(ui: &mut egui::Ui, items: &[StripItem], selected: Option<usize>) -> StripHit {
    ui.horizontal_centered(|ui| strip(ui, visible(items), selected)).inner
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub title: String,
    pub harness_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct TabStrip {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

/// What the bar (or a Cmd-key) asked the host to do. Applied by the host after
/// the frame, alongside the matching session spawn/drop.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAction {
    Switch(usize),
    Close(usize),
    /// Open a new tab running the given harness id.
    New(String),
}

impl TabStrip {
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    /// Append and activate — a new tab is where you're about to work.
    pub fn add(&mut self, tab: Tab) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Remove `i`; returns true while tabs remain. Activation moves to the left
    /// neighbour (closing the rightmost) or stays at the same index otherwise —
    /// the convention every tabbed terminal follows.
    pub fn close(&mut self, i: usize) -> bool {
        if i < self.tabs.len() {
            self.tabs.remove(i);
            if self.active >= self.tabs.len() && self.active > 0 {
                self.active -= 1;
            }
        }
        !self.tabs.is_empty()
    }

    pub fn switch(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
        }
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }
}

/// Map one Cmd-modified key press to a tab action. macOS terminal convention:
/// ⌘ belongs to the terminal (chrome), and is never forwarded to the PTY —
/// `term_view` skips ⌘-keys for exactly this reason, so there is no overlap.
pub fn command_key_action(
    key: egui::Key,
    mods: egui::Modifiers,
    strip: &TabStrip,
    default_harness: &str,
) -> Option<TabAction> {
    if !mods.command {
        return None;
    }
    use egui::Key as K;
    match key {
        K::T => Some(TabAction::New(default_harness.to_string())),
        K::W => Some(TabAction::Close(strip.active)),
        K::CloseBracket if mods.shift => {
            Some(TabAction::Switch((strip.active + 1) % strip.tabs.len().max(1)))
        }
        K::OpenBracket if mods.shift => Some(TabAction::Switch(
            (strip.active + strip.tabs.len().max(1) - 1) % strip.tabs.len().max(1),
        )),
        K::Num1 | K::Num2 | K::Num3 | K::Num4 | K::Num5 | K::Num6 | K::Num7 | K::Num8
        | K::Num9 => {
            let n = match key {
                K::Num1 => 0,
                K::Num2 => 1,
                K::Num3 => 2,
                K::Num4 => 3,
                K::Num5 => 4,
                K::Num6 => 5,
                K::Num7 => 6,
                K::Num8 => 7,
                _ => 8,
            };
            (n < strip.tabs.len()).then_some(TabAction::Switch(n))
        }
        _ => None,
    }
}

/// The tab strip's payload, as data. Each session becomes one chip carrying the glyph
/// of the harness that spawned it; a tab whose harness has left the registry keeps its
/// title and falls back to [`FALLBACK_GLYPH`], because a session that is still running
/// must still be reachable.
pub fn tab_items(tabs: &TabStrip, registry: &[HarnessSpec]) -> Vec<StripItem> {
    tabs.tabs
        .iter()
        .map(|tab| {
            let glyph = registry
                .iter()
                .find(|h| h.id == tab.harness_id)
                .and_then(|h| sole_char(&h.glyph))
                .unwrap_or(FALLBACK_GLYPH);
            StripItem::new(tab.title.clone()).glyph(glyph)
        })
        .collect()
}

/// The tab strip: [`strip`] plus the tab-only extras — the close affordance and the
/// **+** menu. Along the top of the window; returns at most one action.
pub fn tab_bar(
    ui: &mut egui::Ui,
    tabs: &TabStrip,
    registry: &[HarnessSpec],
    installed: &HashSet<String>,
    plus_open: &mut bool,
) -> Option<TabAction> {
    tab_bar_at(ui, StripEdge::Top, tabs, registry, installed, plus_open)
}

/// [`tab_bar`] with the edge spelled out — the host decides where its chrome lives, and
/// the **+** menu follows from that rather than from a hard-coded direction.
pub fn tab_bar_at(
    ui: &mut egui::Ui,
    edge: StripEdge,
    tabs: &TabStrip,
    registry: &[HarnessSpec],
    installed: &HashSet<String>,
    plus_open: &mut bool,
) -> Option<TabAction> {
    let mut action = None;
    let font = egui::FontId::monospace(STRIP_FONT_SIZE);
    let items = tab_items(tabs, registry);
    ui.horizontal_centered(|ui| {
        let hit = strip(ui, &items, Some(tabs.active));
        // Tapping the tab you are already in is not a switch; middle-click is close.
        if let Some(i) = hit.activated.filter(|i| *i != tabs.active) {
            action = Some(TabAction::Switch(i));
        }
        if let Some(i) = hit.dismissed {
            action = Some(TabAction::Close(i));
        }
        let plus = ui.add(
            egui::Button::new(
                egui::RichText::new("+")
                    .font(font.clone())
                    .color(egui::Color32::from_rgb(0x8a, 0x96, 0x8a)),
            )
            .frame(false),
        );
        if plus.clicked() {
            *plus_open = !*plus_open;
        }

        // The + menu, Superconductor-exact in T1 form: numbered, installed
        // selectable, missing greyed with the install URL as hover text.
        if *plus_open {
            egui::Area::new(egui::Id::new("harness-menu"))
                .fixed_pos(edge.menu_origin(plus.rect))
                .pivot(edge.menu_pivot())
                .show(ui.ctx(), |ui| {
                    egui::Frame::menu(ui.style())
                        .fill(egui::Color32::from_rgb(0x10, 0x14, 0x10))
                        .show(ui, |ui| {
                            for (n, h) in registry.iter().enumerate() {
                                let is_installed = installed.contains(&h.id);
                                let label = egui::RichText::new(format!(
                                    "{}  {} {}",
                                    n + 1,
                                    h.glyph,
                                    h.name
                                ))
                                .font(font.clone())
                                .color(if is_installed {
                                    egui::Color32::from_rgb(0xc8, 0xe6, 0xc8)
                                } else {
                                    egui::Color32::from_rgb(0x50, 0x5a, 0x50)
                                });
                                let resp = ui.add_enabled(
                                    is_installed,
                                    egui::Button::new(label).frame(false),
                                );
                                let resp = match (&h.install_url, is_installed) {
                                    (Some(url), false) => {
                                        resp.on_disabled_hover_text(format!("Install: {url}"))
                                    }
                                    _ => resp,
                                };
                                if resp.clicked() {
                                    action = Some(TabAction::New(h.id.clone()));
                                    *plus_open = false;
                                }
                            }
                        });
                });
            // Click-away closes; Escape is the terminal's, not the menu's.
            if ui.ctx().input(|i| i.pointer.any_click()) && !plus.clicked() && action.is_none() {
                *plus_open = false;
            }
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip3() -> TabStrip {
        let mut s = TabStrip::default();
        s.add(Tab { title: "zsh".into(), harness_id: "shell".into() });
        s.add(Tab { title: "Pi".into(), harness_id: "pi".into() });
        s.add(Tab { title: "Claude Code".into(), harness_id: "claude".into() });
        s
    }

    #[test]
    fn add_activates_and_close_moves_left() {
        let mut s = strip3();
        assert_eq!(s.active, 2, "a new tab is where you work");
        assert!(s.close(2));
        assert_eq!(s.active, 1, "closing the rightmost moves left");
        assert!(s.close(0));
        assert_eq!(s.active, 0, "closing before the active shifts it into place");
        assert!(!s.close(0), "closing the last reports empty");
    }

    #[test]
    fn wraparound_next_prev() {
        let mut s = strip3();
        s.switch(2);
        s.next();
        assert_eq!(s.active, 0);
        s.prev();
        assert_eq!(s.active, 2);
    }

    #[test]
    fn command_keys_map_to_actions() {
        let s = strip3();
        let cmd = egui::Modifiers::COMMAND;
        assert_eq!(
            command_key_action(egui::Key::T, cmd, &s, "shell"),
            Some(TabAction::New("shell".into()))
        );
        assert_eq!(
            command_key_action(egui::Key::W, cmd, &s, "shell"),
            Some(TabAction::Close(2))
        );
        assert_eq!(
            command_key_action(egui::Key::Num2, cmd, &s, "shell"),
            Some(TabAction::Switch(1))
        );
        assert_eq!(
            command_key_action(egui::Key::Num9, cmd, &s, "shell"),
            None,
            "a number past the strip is a no-op, not a panic"
        );
        assert_eq!(
            command_key_action(egui::Key::T, egui::Modifiers::CTRL, &s, "shell"),
            None,
            "bare Ctrl belongs to the shell, never the chrome"
        );
    }

    fn items(labels: &[&str]) -> Vec<StripItem> {
        labels.iter().map(|l| StripItem::new(*l)).collect()
    }

    #[test]
    fn payload_past_the_cap_truncates_and_never_scrolls() {
        assert_eq!(strip_fit(3), StripFit { shown: 3, dropped: 0 });
        assert_eq!(strip_fit(STRIP_ITEM_CAP), StripFit { shown: 10, dropped: 0 });
        assert_eq!(
            strip_fit(14),
            StripFit { shown: 10, dropped: 4 },
            "the schema truncates to the cap and reports the rest in `coverage`"
        );
        let many = items(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]);
        let drawn = visible(&many);
        assert_eq!(drawn.len(), STRIP_ITEM_CAP);
        assert_eq!(
            drawn.first().map(|i| i.label.as_str()),
            Some("a"),
            "truncation keeps the emitter's own ranking — it is a prefix, not a reshuffle"
        );
        assert_eq!(drawn.last().map(|i| i.label.as_str()), Some("j"));
    }

    #[test]
    fn an_empty_payload_yields_nothing_to_draw() {
        assert_eq!(strip_fit(0), StripFit { shown: 0, dropped: 0 });
        assert!(visible(&[]).is_empty(), "zero entries hides the strip; it never draws an empty row");
    }

    #[test]
    fn a_chip_says_glyph_then_label() {
        assert_eq!(StripItem::new("Roughness").glyph('◈').text(), "◈ Roughness");
        assert_eq!(StripItem::new("Roughness").text(), "Roughness", "no glyph, no leading space");
        assert!(StripItem::new("x").enabled, "a chip is tappable unless it says otherwise");
        assert!(!StripItem::new("x").enabled(false).enabled);
    }

    #[test]
    fn a_glyph_is_one_character_or_none() {
        assert_eq!(sole_char("π"), Some('π'));
        assert_eq!(sole_char("❯"), Some('❯'));
        assert_eq!(sole_char(""), None);
        assert_eq!(sole_char(">>"), None, "half a glyph is worse than the fallback");
    }

    fn spec(id: &str, glyph: &str) -> HarnessSpec {
        HarnessSpec {
            id: id.into(),
            name: id.into(),
            glyph: glyph.into(),
            command: vec![],
            detect: vec![],
            install_url: None,
            cwd: None,
            wsl: false,
            wsl_distro: None,
        }
    }

    #[test]
    fn tab_items_carry_the_harnesss_glyph_and_fall_back() {
        let s = strip3();
        let registry = vec![spec("shell", "❯"), spec("pi", "π")];
        let drawn = tab_items(&s, &registry);
        assert_eq!(drawn.len(), 3);
        assert_eq!(drawn[0].text(), "❯ zsh");
        assert_eq!(drawn[1].text(), "π Pi");
        assert_eq!(
            drawn[2].text(),
            "❯ Claude Code",
            "a tab whose harness left the registry keeps its title and stays reachable"
        );
        assert!(drawn.iter().all(|i| i.enabled), "every live session is switchable");
    }

    #[test]
    fn a_menu_opens_away_from_its_strips_edge() {
        let control = egui::Rect::from_min_max(egui::pos2(10.0, 4.0), egui::pos2(30.0, 24.0));
        assert_eq!(StripEdge::Top.menu_origin(control), egui::pos2(10.0, 24.0 + MENU_GAP));
        assert_eq!(StripEdge::Top.menu_pivot(), egui::Align2::LEFT_TOP, "top strip: hang down");
        assert_eq!(StripEdge::Bottom.menu_origin(control), egui::pos2(10.0, 4.0 - MENU_GAP));
        assert_eq!(StripEdge::Bottom.menu_pivot(), egui::Align2::LEFT_BOTTOM, "bottom strip: rise");
    }
}
