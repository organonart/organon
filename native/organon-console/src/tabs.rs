//! Tabs (Console #11 Tier 1): the Superconductor model in terminal clothes.
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
///
/// `repeat` is `egui::Event::Key::repeat`: a *held* key streams `pressed: true`
/// events and egui marks every one after the first (`InputState::begin_pass` does
/// `repeat = !first_press` and leaves the event in the stream, so a caller that
/// destructures with `..` sees an unbroken run of fresh presses). The host applies
/// at most one action per frame, which bounds the rate and does not bound the
/// total — autorepeat is slower than the frame rate, so a held chord lands roughly
/// one action per repeat for as long as the finger rests.
///
/// **The policy is a property of the ACTION, not of the key**, which is why the
/// filter is here rather than at the call site and why the match below is
/// exhaustive over [`TabAction`] — a chord added later inherits the right answer,
/// and a *variant* added later fails the build rather than defaulting silently:
///
/// * [`TabAction::Switch`] **honours** a repeat. It is navigation, and this is
///   where a held key earns its stream: ⌘⇧[/] cycling while held is the behaviour
///   you want from a cycle chord, and ⌘1-9 is idempotent to the point of being
///   free — the host answers it with `strip.switch(i)`, one index write, so the
///   thirtieth repeat writes the same number as the first.
/// * [`TabAction::New`] and [`TabAction::Close`] **refuse** it. Both are
///   unbounded and neither is recoverable: `New` spawns a PTY per event, and
///   `Close` drops a session, frees its textures, and quits the console once the
///   last tab goes. A resting finger is not a request for thirty shells, and it is
///   certainly not a request to close thirty tabs.
pub fn command_key_action(
    key: egui::Key,
    mods: egui::Modifiers,
    repeat: bool,
    strip: &TabStrip,
    default_harness: &str,
) -> Option<TabAction> {
    if !mods.command {
        return None;
    }
    use egui::Key as K;
    let action = match key {
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
    };
    match action {
        // Navigation: a held key means "keep going", and repeating a number means
        // nothing at all. Both are safe to stream.
        Some(TabAction::Switch(i)) => Some(TabAction::Switch(i)),
        // Creation and destruction: one press, one action.
        Some(a @ (TabAction::New(_) | TabAction::Close(_))) => (!repeat).then_some(a),
        None => None,
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
            // top panel (`TopBottomPanel::top("tab-strip")` in console_main), so a
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
            command_key_action(egui::Key::T, cmd, false, &s, "shell"),
            Some(TabAction::New("shell".into()))
        );
        assert_eq!(
            command_key_action(egui::Key::W, cmd, false, &s, "shell"),
            Some(TabAction::Close(2))
        );
        assert_eq!(
            command_key_action(egui::Key::Num2, cmd, false, &s, "shell"),
            Some(TabAction::Switch(1))
        );
        assert_eq!(
            command_key_action(egui::Key::Num9, cmd, false, &s, "shell"),
            None,
            "a number past the strip is a no-op, not a panic"
        );
        assert_eq!(
            command_key_action(egui::Key::T, egui::Modifiers::CTRL, false, &s, "shell"),
            None,
            "bare Ctrl belongs to the shell, never the chrome"
        );
    }

    /// The reproduction, driven through a **real** `egui::Context` rather than a
    /// hand-set flag — the claim under test is egui's behaviour, so asserting it
    /// against our own idea of egui would prove nothing. Two frames, one held ⌘T,
    /// no release between: this is the event stream a resting finger produces.
    #[test]
    fn a_held_command_key_reaches_the_frame_loop_as_a_run_of_presses() {
        let ctx = egui::Context::default();
        let press = |modifiers| egui::Event::Key {
            key: egui::Key::T,
            physical_key: None,
            pressed: true,
            // What an integration sends: `egui-winit` discards winit's own repeat
            // flag (`repeat: _, // egui will figure this out for us`) and pushes
            // every autorepeat as a plain press, so `false` here is the truthful
            // input and egui is what fills the flag in.
            repeat: false,
            modifiers,
        };
        let mut seen: Vec<bool> = Vec::new();
        let mut actions = Vec::new();
        let s = strip3();
        for _ in 0..2 {
            let raw = egui::RawInput {
                modifiers: egui::Modifiers::COMMAND,
                events: vec![press(egui::Modifiers::COMMAND)],
                ..Default::default()
            };
            ctx.run(raw, |ctx| {
                ctx.input(|i| {
                    for ev in &i.events {
                        if let egui::Event::Key {
                            key,
                            pressed: true,
                            repeat,
                            modifiers,
                            ..
                        } = ev
                        {
                            seen.push(*repeat);
                            actions.extend(command_key_action(
                                *key, *modifiers, *repeat, &s, "shell",
                            ));
                        }
                    }
                });
            });
        }
        assert_eq!(
            seen,
            vec![false, true],
            "egui marks the second press of a key never released as a repeat, and \
             leaves it in the stream — this is the bug's whole mechanism"
        );
        assert_eq!(
            actions,
            vec![TabAction::New("shell".into())],
            "one finger, one tab: without the filter this held ⌘T is two spawns, \
             and a real hold is one per repeat for as long as it rests"
        );
    }

    #[test]
    fn a_repeat_never_opens_or_closes_a_tab() {
        let s = strip3();
        let cmd = egui::Modifiers::COMMAND;
        assert_eq!(
            command_key_action(egui::Key::T, cmd, true, &s, "shell"),
            None,
            "a held ⌘T is not a request for a shell per repeat"
        );
        assert_eq!(
            command_key_action(egui::Key::W, cmd, true, &s, "shell"),
            None,
            "a held ⌘W would close every tab and then quit the console"
        );
    }

    #[test]
    fn a_repeat_still_navigates_because_navigation_is_what_holding_a_key_is_for() {
        let mut s = strip3();
        s.switch(0);
        let cycle = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        assert_eq!(
            command_key_action(egui::Key::CloseBracket, cycle, true, &s, "shell"),
            Some(TabAction::Switch(1)),
            "a held cycle chord keeps cycling — that is the behaviour, not a defect"
        );
        assert_eq!(
            command_key_action(egui::Key::OpenBracket, cycle, true, &s, "shell"),
            Some(TabAction::Switch(2)),
            "and backwards likewise"
        );
        assert_eq!(
            command_key_action(egui::Key::Num2, egui::Modifiers::COMMAND, true, &s, "shell"),
            Some(TabAction::Switch(1)),
            "a held number is idempotent: the host answers with one index write"
        );
    }
}
