//! Tabs (Shell #11 Tier 1): the Superconductor model in terminal clothes.
//!
//! Each tab is one harness session in its own PTY; the strip along the top is
//! the ONE permitted piece of chrome (PRD v3.1 FR-T11), styled like the founder's
//! Kitty setup — monospace chips on the dark ground, nothing rounded, nothing
//! glossy. The **+** control drops the numbered harness list exactly like the
//! Superconductor screenshot: installed entries selectable, missing ones greyed
//! with their install URL on hover.
//!
//! The model ([`TabStrip`]) is pure and fully tested; the renderer ([`tab_bar`])
//! returns a [`TabAction`] for the host to apply — the strip never mutates itself
//! mid-frame, which is what keeps session lifetimes (spawn/kill) in the host where
//! they belong.

use crate::harness::HarnessSpec;
use crate::theme::Theme;
use std::collections::HashSet;

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

/// The strip itself: chips + the **+** menu. Returns at most one action.
pub fn tab_bar(
    ui: &mut egui::Ui,
    strip: &TabStrip,
    registry: &[HarnessSpec],
    installed: &HashSet<String>,
    plus_open: &mut bool,
    theme: &Theme,
) -> Option<TabAction> {
    let mut action = None;
    let font = egui::FontId::monospace(12.0);
    ui.horizontal_centered(|ui| {
        ui.add_space(6.0);
        for (i, tab) in strip.tabs.iter().enumerate() {
            let active = i == strip.active;
            let glyph = registry
                .iter()
                .find(|h| h.id == tab.harness_id)
                .map(|h| h.glyph.as_str())
                .unwrap_or("❯");
            let text = egui::RichText::new(format!("{glyph} {}", tab.title))
                .font(font.clone())
                .color(if active { theme.tab_active } else { theme.tab_inactive });
            let resp = ui.selectable_label(active, text);
            if resp.clicked() && !active {
                action = Some(TabAction::Switch(i));
            }
            if resp.middle_clicked() {
                action = Some(TabAction::Close(i));
            }
        }
        let plus = ui.add(
            egui::Button::new(
                egui::RichText::new("+")
                    .font(font.clone())
                    .color(theme.tab_plus),
            )
            .frame(false),
        );
        if plus.clicked() {
            *plus_open = !*plus_open;
        }

        // The + menu, Superconductor-exact in T1 form: numbered, installed
        // selectable, missing greyed with the install URL as hover text.
        if *plus_open {
            // Anchored to the button's BOTTOM edge, growing DOWN — the strip is a
            // top panel (`TopBottomPanel::top("tab-strip")` in shell_main), so a
            // menu belongs under it. It used to be `left_top() - 8` with a
            // LEFT_BOTTOM pivot, which grew upward: correct when the strip ran
            // along the bottom of the window, and after the move it put the anchor
            // at roughly y = -8. It still looked right only because egui clamps an
            // `Area` back inside the screen rect — so the position was a fallback,
            // not a placement, and any change to that clamping or to the strip's
            // height would have moved it. Deriving it from the button rather than
            // from the strip keeps it true if the height changes again.
            let below = plus.rect.left_bottom() + egui::vec2(0.0, 8.0);
            egui::Area::new(egui::Id::new("harness-menu"))
                .fixed_pos(below)
                .pivot(egui::Align2::LEFT_TOP)
                .show(ui.ctx(), |ui| {
                    egui::Frame::menu(ui.style())
                        .fill(theme.tab_menu_fill)
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
                                    theme.tab_menu_installed
                                } else {
                                    theme.tab_menu_missing
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
}
