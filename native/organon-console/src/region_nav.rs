//! **Which region a direction means — the keyboard's half of viewport navigation.**
//!
//! `Super + H/J/K/L` moves between windows under Hyprland. This is the same gesture one level
//! in: **`Alt + H/J/K/L` moves between regions inside Organon**, arrows as synonyms. The point
//! is not that Alt happens to be free — measured on Omarchy 4, Super carries 42 binds and every
//! Super combination is saturated while Alt carries 8, all of them `Tab`, `Print` and media keys
//! — it is that the result *composes* with the compositor instead of competing with it. Walk
//! `Super+L` into the Organon window and keep going with `Alt+L` into the region beside it, and
//! the motion does not break at the window boundary.
//!
//! # 🚨 Geometric, never an enum order
//!
//! [`Region`] is a twelve-variant grid and it would be easy to resolve a direction by stepping
//! through [`Region::ALL`]. That would mean something different in every layout — `Right` from
//! `TopLeft` would land somewhere that depends on which regions happen to be occupied rather
//! than on where they are. **A direction is resolved from the rectangles the layout actually
//! drew**, so it means the same thing everywhere and it keeps meaning it when the layout gains
//! a region this file has never heard of.
//!
//! # ⚠️ No wrapping, deliberately
//!
//! A direction with nothing that way is a **no-op**. In a four-region grid, wrapping is how you
//! end up somewhere you did not ask for: the gesture is "go right", and if the answer is
//! "there is nothing to the right" the honest response is to stay put. `region_line.rs`'s
//! completion ring wraps because a ring is a list you are cycling; a room is not.
//!
//! # ⚠️ This file decides nothing about the latch
//!
//! It answers "which region", and the caller decides what arriving there means — a hosted
//! region latches, an agent region focuses its composer. Keeping that split is what stops a
//! geometry helper from growing an opinion about modules.

use crate::region::Region;

/// The four directions, and the only thing a navigation key can mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// **The chord: `Alt` plus a direction, and nothing else.**
///
/// ⚠️ **Any other modifier refuses.** `Alt+Shift` and `Ctrl+Alt` both carry Hyprland binds on
/// Omarchy, and `Super` in every combination is the compositor's — so a chord that fired on
/// "alt is among the modifiers" would answer a keystroke the window manager had already
/// claimed, and the two would both act. The test is `alt && !ctrl && !shift && !command`.
///
/// 📌 **`H/J/K/L` and the arrows are synonyms**, in that order of intent: the letters are what
/// Hyprland binds for window focus, so they are the ones that make the two levels feel like one
/// gesture; the arrows are there because not everybody has the letters in their fingers.
#[must_use]
pub fn direction(key: egui::Key, mods: egui::Modifiers) -> Option<Dir> {
    if !mods.alt || mods.ctrl || mods.shift || mods.command || mods.mac_cmd {
        return None;
    }
    match key {
        egui::Key::H | egui::Key::ArrowLeft => Some(Dir::Left),
        egui::Key::L | egui::Key::ArrowRight => Some(Dir::Right),
        egui::Key::K | egui::Key::ArrowUp => Some(Dir::Up),
        egui::Key::J | egui::Key::ArrowDown => Some(Dir::Down),
        _ => None,
    }
}

/// **The region `dir` reaches from `from`, or `None` if there is nothing that way.**
///
/// Two filters and one ordering, which is the whole algorithm:
///
/// 1. A candidate must be **strictly** in that direction — its far edge past `from`'s near edge.
///    Strictly, so a region merely overlapping on the axis cannot be "to the right" of one it is
///    level with.
/// 2. Candidates are ranked by **perpendicular overlap first**, then by distance. Overlap first
///    is what makes `Right` from a tall left column land in the region beside it rather than in
///    a corner that happens to be marginally closer — the corner is *diagonal*, and a diagonal
///    answer to a cardinal question is the thing that makes directional focus feel arbitrary.
/// 3. `from` itself is never a candidate, matched by rectangle rather than by name so a caller
///    that does not know which region it is standing in still cannot be sent nowhere.
#[must_use]
pub fn target(from: egui::Rect, dir: Dir, candidates: &[(Region, egui::Rect)]) -> Option<Region> {
    let mut best: Option<(Region, f32, f32)> = None;
    for (region, rect) in candidates {
        if *rect == from {
            continue;
        }
        let beyond = match dir {
            Dir::Left => rect.right() <= from.left() + EPS,
            Dir::Right => rect.left() >= from.right() - EPS,
            Dir::Up => rect.bottom() <= from.top() + EPS,
            Dir::Down => rect.top() >= from.bottom() - EPS,
        };
        if !beyond {
            continue;
        }
        let overlap = match dir {
            Dir::Left | Dir::Right => {
                (from.bottom().min(rect.bottom()) - from.top().max(rect.top())).max(0.0)
            }
            Dir::Up | Dir::Down => {
                (from.right().min(rect.right()) - from.left().max(rect.left())).max(0.0)
            }
        };
        let distance = match dir {
            Dir::Left => from.left() - rect.right(),
            Dir::Right => rect.left() - from.right(),
            Dir::Up => from.top() - rect.bottom(),
            Dir::Down => rect.top() - from.bottom(),
        };
        let better = match best {
            None => true,
            // Overlap first, distance second. Equal overlap is common — a clean split has two
            // regions sharing a full edge — so the distance tie-break is the usual path, not
            // an edge case.
            Some((_, bo, bd)) => overlap > bo + EPS || ((overlap - bo).abs() <= EPS && distance < bd),
        };
        if better {
            best = Some((*region, overlap, distance));
        }
    }
    best.map(|(r, _, _)| r)
}

/// Rectangles arrive from a layout that computed them in floating point, so edges that are
/// logically shared are not always bit-identical. Everything here is in points on a real
/// display; half a point is far below anything a person can express and far above the error a
/// layout accumulates.
const EPS: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    }

    /// A plain left/right split, which is what `organon-os` runs: agent left, Ascent right.
    fn split() -> (egui::Rect, Vec<(Region, egui::Rect)>) {
        let left = r(0.0, 0.0, 800.0, 1000.0);
        let right = r(800.0, 0.0, 1600.0, 1000.0);
        (left, vec![(Region::Left, left), (Region::Right, right)])
    }

    #[test]
    fn alt_l_from_the_left_region_reaches_the_right_one() {
        let (from, all) = split();
        assert_eq!(target(from, Dir::Right, &all), Some(Region::Right));
    }

    #[test]
    fn alt_h_from_the_right_region_comes_back() {
        let (_, all) = split();
        let right = all[1].1;
        assert_eq!(target(right, Dir::Left, &all), Some(Region::Left));
    }

    /// 🚨 The no-wrap rule. Going right from the rightmost region is a no-op, not a jump to the
    /// left one — see the module header.
    #[test]
    fn there_is_no_wrapping() {
        let (_, all) = split();
        let right = all[1].1;
        assert_eq!(target(right, Dir::Right, &all), None);
        let left = all[0].1;
        assert_eq!(target(left, Dir::Left, &all), None);
    }

    /// A cardinal question never gets a diagonal answer, even when the diagonal is closer.
    /// `TopRight` here is nearer by centre distance than `Right`, and `Right` is the answer
    /// because it is the one this rectangle is level with.
    #[test]
    fn overlap_beats_distance_so_right_is_never_a_corner() {
        let from = r(0.0, 400.0, 400.0, 600.0);
        let all = vec![
            (Region::Right, r(500.0, 400.0, 900.0, 600.0)),
            (Region::TopRight, r(420.0, 0.0, 900.0, 200.0)),
        ];
        assert_eq!(target(from, Dir::Right, &all), Some(Region::Right));
    }

    /// With equal overlap the nearer one wins — the ordinary case in a three-column layout.
    #[test]
    fn equal_overlap_falls_back_to_distance() {
        let from = r(0.0, 0.0, 100.0, 1000.0);
        let all = vec![
            (Region::Right, r(600.0, 0.0, 900.0, 1000.0)),
            (Region::Center, r(200.0, 0.0, 500.0, 1000.0)),
        ];
        assert_eq!(target(from, Dir::Right, &all), Some(Region::Center));
    }

    #[test]
    fn a_region_is_never_its_own_target() {
        let (from, all) = split();
        assert_eq!(target(from, Dir::Left, &all), None);
    }

    #[test]
    fn vertical_works_the_same_way() {
        let top = r(0.0, 0.0, 1000.0, 400.0);
        let bottom = r(0.0, 400.0, 1000.0, 800.0);
        let all = vec![(Region::Top, top), (Region::Bottom, bottom)];
        assert_eq!(target(top, Dir::Down, &all), Some(Region::Bottom));
        assert_eq!(target(bottom, Dir::Up, &all), Some(Region::Top));
        assert_eq!(target(top, Dir::Up, &all), None);
    }

    #[test]
    fn the_letters_and_the_arrows_are_synonyms() {
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        for (key, dir) in [
            (egui::Key::H, Dir::Left),
            (egui::Key::ArrowLeft, Dir::Left),
            (egui::Key::L, Dir::Right),
            (egui::Key::ArrowRight, Dir::Right),
            (egui::Key::K, Dir::Up),
            (egui::Key::ArrowUp, Dir::Up),
            (egui::Key::J, Dir::Down),
            (egui::Key::ArrowDown, Dir::Down),
        ] {
            assert_eq!(direction(key, alt), Some(dir), "{key:?}");
        }
    }

    /// 🚨 Any extra modifier refuses, because Hyprland has already claimed those chords and two
    /// handlers acting on one keystroke is the failure this guard exists to prevent.
    #[test]
    fn a_second_modifier_refuses() {
        for mods in [
            egui::Modifiers { alt: true, shift: true, ..Default::default() },
            egui::Modifiers { alt: true, ctrl: true, ..Default::default() },
            egui::Modifiers { alt: true, command: true, ..Default::default() },
            egui::Modifiers::default(),
            egui::Modifiers { ctrl: true, ..Default::default() },
        ] {
            assert_eq!(direction(egui::Key::L, mods), None, "{mods:?}");
        }
    }

    #[test]
    fn an_unrelated_key_is_not_a_direction() {
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        for key in [egui::Key::A, egui::Key::Tab, egui::Key::Escape, egui::Key::Num1] {
            assert_eq!(direction(key, alt), None, "{key:?}");
        }
    }
}
