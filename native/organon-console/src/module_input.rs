//! **The console's half of the input grant — who is flying, and what reaches them.**
//!
//! [`organon_module::input`] owns the *protocol*: four events, a published set of keys that can
//! never be delivered, and the refusal table saying what a hosted module may never be handed.
//! That half has been merged since T5b's contract landed and **nothing on the console side ever
//! spoke it** — a producer could draw a picture and had no way to be driven. This file is the
//! missing side: it turns what egui saw into those four events, and it owns the latch that says
//! whether a rectangle is being flown or merely looked at.
//!
//! # 🚨 The latch, and why the way out came first
//!
//! `doc/organon_module_viewport.md` §5.3: *"the way out must be decided before the way in is
//! built"*, and *"whatever is chosen must be a key the module is **told** it will never receive,
//! rather than a key we hope it ignores."* Both halves are already true —
//! [`organon_module::input::RESERVED`] is `Escape` and `F11`, enforced at the **encode** site so
//! a console that forgot could not leak them, and published in the mapped header so a module
//! that does not link the crate is still *told*. This file spends that: **`Escape` leaves.**
//!
//! ⚠️ **The console must therefore never send `Escape`, and never needs to check.** It is not
//! that [`translate`] filters it out as a courtesy — [`organon_module::input::push`] refuses it,
//! so the guarantee holds even against a bug here. What this file does with `Escape` is
//! *release the latch*, which is a console action and not a wire event.
//!
//! # What the latch is for
//!
//! A pointer over a rectangle is ambiguous: the console has its own scroll, its own click
//! targets, and a region divider. So a rectangle is **flown only while latched**, a click
//! inside latches it, and everything below is gated on that. ⚠️ The latch is not a claim on the
//! *window* — it is one producer's name, so a second hosted region cannot also be flying and
//! the answer to "who has the keys" is one `Option`, never a set.
//!
//! ⚠️ **Releasing always emits [`InputEvent::ReleaseAll`]**, and the three ways to release are
//! `Escape`, losing window focus, and the region ceasing to hold that producer. All three are
//! the same hazard: a key that went down inside the latch and whose `Up` the module will never
//! see, leaving it thrusting forever with nobody touching the keyboard. [`Latch::release`] is
//! the only way to leave, so the emit cannot be forgotten at one of the three sites.

use organon_module::{Button, InputEvent, Key, MouseButton, RESERVED};

/// **Who is being flown.** At most one producer, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Latch(Option<String>);

impl Latch {
    /// Nobody is flying.
    pub fn new() -> Self {
        Self(None)
    }

    /// The producer currently flown, if any.
    pub fn held(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Is this producer the one being flown?
    pub fn is(&self, producer: &str) -> bool {
        self.0.as_deref() == Some(producer)
    }

    /// Take the latch for `producer`.
    ///
    /// ⚠️ **Latching a second producer releases the first**, and answers the release so the
    /// caller can put it on the outgoing producer's wire. Silently moving the latch would leave
    /// whatever the first module was holding held forever — the exact hazard
    /// [`InputEvent::ReleaseAll`] exists for, reached by the one route that looks like a state
    /// change rather than an exit.
    #[must_use = "the displaced producer must be sent ReleaseAll"]
    pub fn latch(&mut self, producer: &str) -> Option<String> {
        if self.is(producer) {
            return None;
        }
        let displaced = self.0.replace(producer.to_string());
        displaced
    }

    /// Let go, answering who was flying so the caller can send them [`InputEvent::ReleaseAll`].
    ///
    /// 🚨 **The only way out**, so that the release event cannot be forgotten at one of the
    /// three sites that need it (`Escape`, focus loss, the region no longer holding it).
    #[must_use = "the released producer must be sent ReleaseAll"]
    pub fn release(&mut self) -> Option<String> {
        self.0.take()
    }
}

/// One thing the console observed this frame, in the vocabulary of egui rather than of the wire.
///
/// ⚠️ **A separate type rather than `egui::Event`, and it is not ceremony.** [`translate`] is
/// where the refusal table becomes code, so it is the function that most needs to be tested
/// against hostile input — a text event, a reserved key, a pointer warp. Taking egui's own enum
/// would mean building `egui::Event` values in every test and would drag the console's whole
/// input surface into the blast radius of this file. This is the subset the grant permits, and
/// building one is the call site's job.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seen {
    /// A key went down or came up. `None` for a key with no wire spelling.
    Key { key: Option<Key>, down: bool },
    /// A mouse button went down or came up.
    Mouse { button: MouseButton, down: bool },
    /// The pointer moved, in points.
    Motion { dx: f32, dy: f32 },
}

/// **Turn what the console saw into what the wire carries.**
///
/// ⚠️ **Motion is coalesced into one event and everything else keeps its order.** egui delivers
/// many small pointer deltas per frame and a module cares about the sum, not the path; a key
/// press and release in the same frame, by contrast, are two facts whose order is the whole
/// meaning. So motion accumulates and is emitted **last**, after every button transition — which
/// also makes a click-then-drag arrive as "button down, then moved", never the reverse.
///
/// 🚨 **A reserved key is dropped here AND refused at the encode site.** The second is the
/// guarantee; this one exists so the console does not spend a ring slot on an event that cannot
/// be delivered. Never rely on this one alone — see the module docs.
///
/// A `Seen::Key` with no wire spelling is dropped: the key map is deliberately partial, and a
/// key the protocol cannot name is not an error.
pub fn translate(seen: &[Seen]) -> Vec<InputEvent> {
    let mut out = Vec::with_capacity(seen.len());
    let (mut dx, mut dy) = (0.0f32, 0.0f32);
    for s in seen {
        match *s {
            Seen::Key { key: Some(k), down } => {
                if RESERVED.contains(&k) {
                    continue;
                }
                let b = Button::Key(k);
                out.push(if down { InputEvent::Down(b) } else { InputEvent::Up(b) });
            }
            Seen::Key { key: None, .. } => {}
            Seen::Mouse { button, down } => {
                let b = Button::Mouse(button);
                out.push(if down { InputEvent::Down(b) } else { InputEvent::Up(b) });
            }
            Seen::Motion { dx: x, dy: y } => {
                dx += x;
                dy += y;
            }
        }
    }
    // Exactly zero motion produces no event: a frame in which the pointer did not move should
    // cost the ring nothing, and `Pointer { dx: 0.0, dy: 0.0 }` is a message that says nothing.
    if dx != 0.0 || dy != 0.0 {
        out.push(InputEvent::Pointer { dx, dy });
    }
    out
}

/// The wire spelling of an egui key, or `None` if the protocol cannot name it.
///
/// ⚠️ **Partial on purpose, and the gaps are the refusal table showing through.** The protocol's
/// `Key` list is USB HID's, which has no notion of a composed character — and
/// [`organon_module::input`] refuses text, IME and characters outright. So this maps the keys a
/// 6DOF instrument is flown with (letters, digits, arrows, space, the modifiers) and declines to
/// invent spellings for the rest.
///
/// 📌 `Escape` and `F11` **do** have spellings here and are mapped like any other key. Dropping
/// them is [`translate`]'s job, done against [`organon_module::input::RESERVED`] itself rather
/// than by omission — a hole in a map is indistinguishable from an oversight, whereas a filter
/// naming the list it filters against says why.
pub fn key_of(k: egui::Key) -> Option<Key> {
    use egui::Key as E;
    Some(match k {
        E::A => Key::A, E::B => Key::B, E::C => Key::C, E::D => Key::D,
        E::E => Key::E, E::F => Key::F, E::G => Key::G, E::H => Key::H,
        E::I => Key::I, E::J => Key::J, E::K => Key::K, E::L => Key::L,
        E::M => Key::M, E::N => Key::N, E::O => Key::O, E::P => Key::P,
        E::Q => Key::Q, E::R => Key::R, E::S => Key::S, E::T => Key::T,
        E::U => Key::U, E::V => Key::V, E::W => Key::W, E::X => Key::X,
        E::Y => Key::Y, E::Z => Key::Z,
        E::Num0 => Key::Digit0, E::Num1 => Key::Digit1, E::Num2 => Key::Digit2,
        E::Num3 => Key::Digit3, E::Num4 => Key::Digit4, E::Num5 => Key::Digit5,
        E::Num6 => Key::Digit6, E::Num7 => Key::Digit7, E::Num8 => Key::Digit8,
        E::Num9 => Key::Digit9,
        E::Enter => Key::Enter,
        E::Escape => Key::Escape,
        E::Backspace => Key::Backspace,
        E::Tab => Key::Tab,
        E::Space => Key::Space,
        E::Minus => Key::Minus,
        E::Equals => Key::Equal,
        E::OpenBracket => Key::BracketLeft,
        E::CloseBracket => Key::BracketRight,
        E::Backslash => Key::Backslash,
        E::Semicolon => Key::Semicolon,
        E::Quote => Key::Quote,
        E::Backtick => Key::Backquote,
        E::Comma => Key::Comma,
        E::Period => Key::Period,
        E::Slash => Key::Slash,
        E::Insert => Key::Insert,
        E::Home => Key::Home,
        E::PageUp => Key::PageUp,
        E::Delete => Key::Delete,
        E::End => Key::End,
        E::PageDown => Key::PageDown,
        E::ArrowRight => Key::ArrowRight,
        E::ArrowLeft => Key::ArrowLeft,
        E::ArrowDown => Key::ArrowDown,
        E::ArrowUp => Key::ArrowUp,
        E::F1 => Key::F1, E::F2 => Key::F2, E::F3 => Key::F3, E::F4 => Key::F4,
        E::F5 => Key::F5, E::F6 => Key::F6, E::F7 => Key::F7, E::F8 => Key::F8,
        E::F9 => Key::F9, E::F10 => Key::F10, E::F11 => Key::F11, E::F12 => Key::F12,
        _ => return None,
    })
}

/// The wire spelling of an egui pointer button.
///
/// ⚠️ egui's `Extra1`/`Extra2` are the side buttons; the protocol calls them `Back`/`Forward`,
/// which is what every browser and every game means by them.
pub fn button_of(b: egui::PointerButton) -> Option<MouseButton> {
    Some(match b {
        egui::PointerButton::Primary => MouseButton::Left,
        egui::PointerButton::Middle => MouseButton::Middle,
        egui::PointerButton::Secondary => MouseButton::Right,
        egui::PointerButton::Extra1 => MouseButton::Back,
        egui::PointerButton::Extra2 => MouseButton::Forward,
    })
}

/// What one frame's raw input yielded to a flown module, and whether the flight ended.
#[derive(Debug, Default, PartialEq)]
pub struct Taken {
    /// What to hand [`translate`].
    pub seen: Vec<Seen>,
    /// The latch must be released — `Escape`, or the window losing focus.
    pub release: bool,
}

/// **Take a flown module's input out of the frame's RAW input, in place.**
///
/// 🚨 **`RawInput`, never `Context::input_mut`, and the difference is not stylistic — it is the
/// bug this function exists as a named, testable unit because of.** `Context::run(raw, …)` calls
/// `InputState::begin_pass`, which builds the frame's state with `events: new.events.clone()`
/// and `focused: new.focused` **straight from the incoming `RawInput`** (egui 0.33.3,
/// `input_state/mod.rs:571`). So anything done to `Context`'s input *before* `run` is discarded
/// wholesale: the first cut of this stole keys from last frame's leftovers, the composer went on
/// receiving every keystroke it was supposed to be shielded from, and the module was fed events
/// one frame stale. Caught in review on PR #207.
///
/// 📌 **Which is why this takes `&mut Vec<egui::Event>` rather than a `Context`.** A function
/// over the event list can be tested against a hostile frame; a function that reaches into a
/// live `Context` can only be tested by running one.
///
/// # What is taken, what is shared, what is never sent
///
/// * **Keys are TAKEN.** While a rectangle is flown, `W` is thrust and not a character, and the
///   composer must never see it. This is the whole reason the arbitration happens before any
///   widget reads.
/// * ⚠️ **Buttons are SHARED — copied, not stolen — and that is load-bearing.** Consuming them
///   would make two documented things impossible at once: clicking a *different* hosted
///   rectangle (one of the four exits, which would become an unreachable exit pretending to be a
///   design), and reaching the console's own chrome while a module is flying. So the module gets
///   them and egui keeps them.
/// * ⚠️ **Motion comes from `MouseMoved`, which is already a delta.** `PointerMoved` carries an
///   absolute position, is never sent, and is **left in the list** because egui needs it for its
///   own hover state. That is the refusal table's rule getting easier rather than harder: there
///   is no conversion step in which an absolute position could leak.
/// * 🚨 **`Escape` is taken and never forwarded.** It ends the flight. `translate` would drop it
///   and `input::push` would refuse it; this is the guard that gives a person their keyboard
///   back.
pub fn take_from_raw(events: &mut Vec<egui::Event>, focused: bool) -> Taken {
    let mut out = Taken::default();
    if !focused {
        out.release = true;
        return out;
    }
    let mut keep = Vec::with_capacity(events.len());
    for ev in events.drain(..) {
        match &ev {
            egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                out.release = true;
            }
            egui::Event::Key { key, pressed, .. } => {
                out.seen.push(Seen::Key { key: key_of(*key), down: *pressed });
            }
            egui::Event::PointerButton { button, pressed, .. } => {
                if let Some(b) = button_of(*button) {
                    out.seen.push(Seen::Mouse { button: b, down: *pressed });
                }
                keep.push(ev);
            }
            egui::Event::MouseMoved(d) => {
                out.seen.push(Seen::Motion { dx: d.x, dy: d.y });
            }
            _ => keep.push(ev),
        }
    }
    *events = keep;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nobody_is_flying_to_begin_with() {
        let l = Latch::new();
        assert_eq!(l.held(), None);
        assert!(!l.is("ascent"));
    }

    #[test]
    fn a_click_takes_the_latch_and_escape_gives_it_back() {
        let mut l = Latch::new();
        assert_eq!(l.latch("ascent"), None, "nothing was displaced");
        assert!(l.is("ascent"));
        assert_eq!(l.release(), Some("ascent".to_string()), "release names who was flying");
        assert_eq!(l.held(), None);
    }

    /// 🚨 **Latching the SAME producer twice must not report a displacement**, or every frame a
    /// pointer is held down would tell the caller to send `ReleaseAll` to the module currently
    /// being flown — which is a stuck-key fix that causes the stuck key.
    #[test]
    fn re_latching_the_same_producer_displaces_nobody() {
        let mut l = Latch::new();
        let _ = l.latch("ascent");
        assert_eq!(l.latch("ascent"), None);
        assert_eq!(l.latch("ascent"), None);
        assert!(l.is("ascent"));
    }

    /// …and latching a DIFFERENT one hands back the first, so its held keys can be released.
    #[test]
    fn latching_another_producer_hands_back_the_first() {
        let mut l = Latch::new();
        let _ = l.latch("ascent");
        assert_eq!(l.latch("descent"), Some("ascent".to_string()));
        assert!(l.is("descent"));
    }

    /// Releasing when nobody is flying is not an error and emits nothing.
    #[test]
    fn releasing_an_empty_latch_names_nobody() {
        let mut l = Latch::new();
        assert_eq!(l.release(), None);
    }

    /// 🚨 **The reserved keys never become wire events.** This is the second of two guards —
    /// `input::push` refuses them regardless — and it is here so a latched module cannot even
    /// cost a ring slot for a key it may not have.
    #[test]
    fn a_reserved_key_never_becomes_an_event() {
        for k in RESERVED {
            let out = translate(&[
                Seen::Key { key: Some(*k), down: true },
                Seen::Key { key: Some(*k), down: false },
            ]);
            assert!(out.is_empty(), "{k:?} reached the wire");
        }
        // …and Escape really is one of them, which is what makes the leave gesture safe.
        assert!(RESERVED.contains(&Key::Escape), "Escape is not reserved — the way out is gone");
    }

    /// A key the protocol cannot name is dropped rather than guessed at.
    #[test]
    fn an_unnameable_key_is_dropped() {
        assert!(translate(&[Seen::Key { key: None, down: true }]).is_empty());
    }

    /// 🚨 **Motion is summed into ONE event and lands after every button transition.** A module
    /// cares about the frame's displacement, not egui's sampling of it — and a click-then-drag
    /// must arrive as "down, then moved" or a drag begins before the button it belongs to.
    #[test]
    fn motion_is_coalesced_and_ordered_after_the_buttons() {
        let out = translate(&[
            Seen::Motion { dx: 1.0, dy: 2.0 },
            Seen::Mouse { button: MouseButton::Left, down: true },
            Seen::Motion { dx: 0.5, dy: -1.0 },
        ]);
        assert_eq!(
            out,
            vec![
                InputEvent::Down(Button::Mouse(MouseButton::Left)),
                InputEvent::Pointer { dx: 1.5, dy: 1.0 },
            ]
        );
    }

    /// A frame in which nothing moved costs the ring nothing.
    #[test]
    fn a_still_pointer_sends_no_motion() {
        let out = translate(&[Seen::Motion { dx: 0.0, dy: 0.0 }]);
        assert!(out.is_empty(), "a zero delta was put on the wire: {out:?}");
    }

    /// Key order is preserved, because down-then-up and up-then-down are different facts.
    #[test]
    fn key_transitions_keep_their_order() {
        let out = translate(&[
            Seen::Key { key: Some(Key::W), down: true },
            Seen::Key { key: Some(Key::A), down: true },
            Seen::Key { key: Some(Key::W), down: false },
        ]);
        assert_eq!(
            out,
            vec![
                InputEvent::Down(Button::Key(Key::W)),
                InputEvent::Down(Button::Key(Key::A)),
                InputEvent::Up(Button::Key(Key::W)),
            ]
        );
    }

    /// 🚨 **The keys a 6DOF instrument is actually flown with all have a spelling.** This is the
    /// test that would have caught a map that compiled and could not fly: thrust, strafe, roll,
    /// and the modifiers a boost or a brake is usually bound to.
    #[test]
    fn everything_you_fly_with_has_a_wire_spelling() {
        use egui::Key as E;
        for k in [
            E::W, E::A, E::S, E::D, E::Q, E::E, E::R, E::F, E::Space, E::Tab,
            E::ArrowUp, E::ArrowDown, E::ArrowLeft, E::ArrowRight,
            E::Num1, E::Num2, E::Num3,
        ] {
            assert!(key_of(k).is_some(), "{k:?} cannot be sent — a module cannot be flown with it");
        }
    }

    /// The map must not collapse two different keys onto one spelling — a transposition here
    /// would be invisible in every test that only checks `is_some`.
    #[test]
    fn the_key_map_is_injective() {
        use std::collections::HashMap;
        let mut seen: HashMap<u16, egui::Key> = HashMap::new();
        for k in egui::Key::ALL {
            if let Some(w) = key_of(*k) {
                if let Some(prev) = seen.insert(w.to_wire(), *k) {
                    panic!("{k:?} and {prev:?} both map to wire 0x{:02x}", w.to_wire());
                }
            }
        }
        assert!(seen.len() > 40, "the map got smaller than a keyboard: {}", seen.len());
    }

    fn key(k: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key: k,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(b: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: egui::pos2(0.0, 0.0),
            button: b,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// 🚨 **The regression this function was extracted for.** A flown module must take the keys
    /// OUT of the frame's event list, or the composer receives them too — `W` typing into the
    /// prompt while it is also thrust. The first cut mutated `Context::input_mut` before
    /// `Context::run`, which egui discards (`begin_pass` rebuilds from the incoming `RawInput`),
    /// so this held for the module and not for the composer.
    #[test]
    fn a_flown_module_takes_the_keys_out_of_the_frame() {
        let mut evs = vec![key(egui::Key::W, true), key(egui::Key::W, false)];
        let t = take_from_raw(&mut evs, true);
        assert!(evs.is_empty(), "the composer would still see: {evs:?}");
        assert_eq!(
            t.seen,
            vec![
                Seen::Key { key: Some(Key::W), down: true },
                Seen::Key { key: Some(Key::W), down: false },
            ]
        );
        assert!(!t.release);
    }

    /// ⚠️ **Buttons are SHARED, not stolen**, and this is the test that keeps two documented
    /// behaviours reachable: clicking a *different* hosted rectangle is one of the four exits,
    /// and the console's chrome has to stay clickable while something is flying. Consuming them
    /// would turn a documented exit into an unreachable one.
    #[test]
    fn buttons_reach_the_module_and_stay_in_the_frame() {
        let mut evs = vec![click(egui::PointerButton::Primary, true)];
        let t = take_from_raw(&mut evs, true);
        assert_eq!(evs.len(), 1, "egui lost the click, so nothing else can be clicked");
        assert_eq!(t.seen, vec![Seen::Mouse { button: MouseButton::Left, down: true }]);
    }

    /// 🚨 **Escape ends the flight and is never forwarded**, which is the whole leave gesture.
    #[test]
    fn escape_releases_and_is_not_sent() {
        let mut evs = vec![key(egui::Key::Escape, true)];
        let t = take_from_raw(&mut evs, true);
        assert!(t.release, "Escape did not end the flight");
        assert!(t.seen.is_empty(), "Escape was queued for the wire: {:?}", t.seen);
        assert!(evs.is_empty(), "Escape was left for the composer");
    }

    /// Losing focus ends the flight, and takes nothing — there is no frame to take it from.
    #[test]
    fn losing_focus_releases_and_takes_nothing() {
        let mut evs = vec![key(egui::Key::W, true)];
        let t = take_from_raw(&mut evs, false);
        assert!(t.release);
        assert!(t.seen.is_empty());
        assert_eq!(evs.len(), 1, "an unfocused frame should not be stripped");
    }

    /// 🚨 **Motion comes from `MouseMoved` (a delta) and `PointerMoved` (absolute) is never
    /// sent** — and is left in the list, because egui needs it for its own hover state. The
    /// refusal table forbids an absolute position crossing; taking the delta directly means
    /// there is no conversion in which one could leak.
    #[test]
    fn motion_is_the_delta_and_the_absolute_position_never_crosses() {
        let mut evs = vec![
            egui::Event::PointerMoved(egui::pos2(400.0, 300.0)),
            egui::Event::MouseMoved(egui::vec2(3.0, -2.0)),
        ];
        let t = take_from_raw(&mut evs, true);
        assert_eq!(t.seen, vec![Seen::Motion { dx: 3.0, dy: -2.0 }]);
        assert_eq!(evs.len(), 1, "PointerMoved must stay for egui's hover state");
        assert!(matches!(evs[0], egui::Event::PointerMoved(_)));
        // …and nothing in what crosses can carry the position it was taken beside.
        for s in &t.seen {
            if let Seen::Motion { dx, dy } = s {
                assert!(*dx != 400.0 && *dy != 300.0, "an absolute position crossed");
            }
        }
    }

    /// Text is refused by the protocol and must not even be gathered — it is left for egui,
    /// which is where a paste into the composer belongs.
    #[test]
    fn text_is_never_gathered() {
        let mut evs = vec![egui::Event::Text("hello".into())];
        let t = take_from_raw(&mut evs, true);
        assert!(t.seen.is_empty(), "text was gathered for a module: {:?}", t.seen);
        assert_eq!(evs.len(), 1, "text was stolen from the composer");
    }

    /// Every mouse button egui can report has a spelling — there are five on each side and a
    /// gap would silently drop a bound control.
    #[test]
    fn every_pointer_button_has_a_spelling() {
        use egui::PointerButton as P;
        let all = [P::Primary, P::Secondary, P::Middle, P::Extra1, P::Extra2];
        let mut wires: Vec<u8> = all.iter().filter_map(|b| button_of(*b)).map(|m| m as u8).collect();
        wires.sort_unstable();
        wires.dedup();
        assert_eq!(wires.len(), all.len(), "two egui buttons share one wire spelling");
    }
}
