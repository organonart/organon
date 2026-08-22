//! **Input — and, more importantly, what this protocol refuses to carry.**
//!
//! `doc/organon_modules_plan.md` §10: *the protocol is the permission set.* A hosted module
//! can do exactly what the protocol lets it do, so **every verb added here is a grant**, and
//! the list of things that are absent is a more load-bearing part of this file than the list
//! of things that are present.
//!
//! # What crosses
//!
//! Four events, and they are four because Ascent's `view::InputEvent` is four:
//! `Down(Source)`, `Up(Source)`, `Pointer { dx, dy }` and `ReleaseAll`. 📌 **The contract
//! carries exactly what the first real producer's own API accepts, and not one verb more.**
//! Designing a richer input vocabulary "while we are here" would be granting capabilities
//! nothing has asked for, to a process boundary whose whole purpose is that it grants
//! narrowly — and `region.rs`'s rule applies to protocols as much as to enums: an
//! unreachable verb is an untested grant pretending to be a design.
//!
//! # 🚨 What does NOT cross, and why each one is a refusal rather than an omission
//!
//! | Absent | Why |
//! |---|---|
//! | **Text, characters, IME** | A stream of what a person typed is keylogger-shaped, and a 6DOF instrument does not need it. If a module ever needs text it is a **new grant with its own name**, argued on its own. |
//! | **Absolute pointer position, and pointer warp** | The console owns the pointer. A producer that could place the cursor could place it over a confirmation button in some other window. Motion is a delta and nothing else. |
//! | **Clipboard, either way** | Reading it is exfiltration; writing it is injection. |
//! | **File paths, drag-and-drop** | A path is a handle to the filesystem, arriving as data the console appeared to vouch for. |
//! | **Raw OS handles** — window handles, surfaces, shared-texture handles | That is mechanism A arriving through the input channel, unmeasured and ungated. §4.4 says when A gets bought, and it is not here. |
//! | **Audio, in either direction** | §5.2. There is deliberately no path, and the design says out loud that this is *promised, not enforced* — a separate process can open WASAPI itself and Ascent already does. The grant is honoured, not prevented. |
//! | 🚨 **A generic message / opaque byte payload** | The one that matters most. An escape hatch makes every future verb free — which is to say **ungranted, for ever**. A verb that can carry any verb is a grant of everything, and it is the single change to this file that would quietly undo §10. |
//!
//! # 🚨 The way out is a key the module is told it will never receive
//!
//! §5.3: *"the way out must be decided before the way in is built"*, and *"whatever is chosen
//! must be a key the module is **told** it will never receive, rather than a key we hope it
//! ignores."*
//!
//! [`RESERVED`] is that promise, and [`InputEvent::is_deliverable`] is where it is kept —
//! at the **encode** site, so a console that forgets cannot leak the key. `Escape` is on it,
//! because a game that swallows Escape and a console that needs Escape are the same key and
//! this is the first place they meet.
//!
//! 🚨 **And this list is PUBLISHED, not merely enforced** — [`crate::wire`]'s
//! `OFF_RESERVED_KEYS` carries it in the mapped header, because a `pub const` tells only the
//! modules that link this crate and a hosted module deliberately does not link it. §5.3 asks
//! for a key the module is *told* it will never receive; a constant only tells the readers who
//! share a compilation. `Header::plan` publishes this exact list, and `wire::tests` pins the
//! two equal so they cannot become two lists.
//!
//! ✏️ **`F11` is on it too, decided rather than defaulted, and the argument is worth keeping
//! because it is what makes the set a safety property rather than a convenience.** §1.12's
//! `console screen` is a **shipped way out of a console filling the display**, so a module
//! that swallowed F11 could strand a person with no keyboard route back — where `Escape` at
//! least has a pointer equivalent in the chrome. That is a falsifier, not a preference: a key
//! whose loss can trap somebody is not a taste question.
//!
//! The objection against was that the console's gesture only applies while the console has
//! focus, which a latched viewport arguably does not have. It does not survive: that is the
//! same objection already overruled for `Escape`, and if the latch changes who owns a key then
//! it wants **one decision for the whole set** rather than a key-by-key drift.
//!
//! ⚠️ **The standing objection that decided the SIZE of the set still stands** — reserving a
//! key costs every module that key for ever — which is why it closes at two rather than
//! growing to "keys the console uses". The membership rule is narrow: *a way out of a state a
//! module could otherwise trap somebody in*, and nothing else qualifies today.

use std::sync::atomic::Ordering;

use crate::map::Mapping;
use crate::wire::{input_ring, Header};

/// Generate the key vocabulary and its wire mapping from one table, so a variant cannot exist
/// without a code and a code cannot exist without a round trip.
macro_rules! keys {
    ($($name:ident = $code:expr),* $(,)?) => {
        /// A physical key, numbered by its **USB HID usage ID** (usage page 0x07).
        ///
        /// 📌 Borrowing an external numbering rather than inventing one is the whole reason
        /// this is safe to put on a wire: the values are fixed by a specification neither
        /// repository controls, so no refactor on either side can renumber them, and a person
        /// debugging a hex dump can look a code up.
        ///
        /// ⚠️ It is a **closed set**. A key not in the table cannot be expressed, which means
        /// a producer never receives one — deliberately, since "everything the OS reports" is
        /// a much larger grant than "the keys an instrument is driven with", and it is the
        /// larger grant that would include whatever a future OS decides to report.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Key { $($name = $code),* }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$name),*];

            pub const fn to_wire(self) -> u16 { self as u16 }

            pub fn from_wire(v: u16) -> Option<Key> {
                match v { $($code => Some(Key::$name),)* _ => None }
            }
        }
    };
}

keys! {
    A = 0x04, B = 0x05, C = 0x06, D = 0x07, E = 0x08, F = 0x09, G = 0x0A, H = 0x0B,
    I = 0x0C, J = 0x0D, K = 0x0E, L = 0x0F, M = 0x10, N = 0x11, O = 0x12, P = 0x13,
    Q = 0x14, R = 0x15, S = 0x16, T = 0x17, U = 0x18, V = 0x19, W = 0x1A, X = 0x1B,
    Y = 0x1C, Z = 0x1D,
    Digit1 = 0x1E, Digit2 = 0x1F, Digit3 = 0x20, Digit4 = 0x21, Digit5 = 0x22,
    Digit6 = 0x23, Digit7 = 0x24, Digit8 = 0x25, Digit9 = 0x26, Digit0 = 0x27,
    Enter = 0x28, Escape = 0x29, Backspace = 0x2A, Tab = 0x2B, Space = 0x2C,
    Minus = 0x2D, Equal = 0x2E, BracketLeft = 0x2F, BracketRight = 0x30, Backslash = 0x31,
    Semicolon = 0x33, Quote = 0x34, Backquote = 0x35, Comma = 0x36, Period = 0x37,
    Slash = 0x38, CapsLock = 0x39,
    F1 = 0x3A, F2 = 0x3B, F3 = 0x3C, F4 = 0x3D, F5 = 0x3E, F6 = 0x3F,
    F7 = 0x40, F8 = 0x41, F9 = 0x42, F10 = 0x43, F11 = 0x44, F12 = 0x45,
    Insert = 0x49, Home = 0x4A, PageUp = 0x4B, Delete = 0x4C, End = 0x4D, PageDown = 0x4E,
    ArrowRight = 0x4F, ArrowLeft = 0x50, ArrowDown = 0x51, ArrowUp = 0x52,
    ControlLeft = 0xE0, ShiftLeft = 0xE1, AltLeft = 0xE2, SuperLeft = 0xE3,
    ControlRight = 0xE4, ShiftRight = 0xE5, AltRight = 0xE6, SuperRight = 0xE7,
}

/// 🚨 Keys the console promises never to deliver. See the module docs.
///
/// **Two, and each is a way *out* of a state a module could otherwise trap somebody in.** That
/// is the whole membership rule, and it is narrower than "keys the console uses": `Escape`
/// leaves the interaction latch, `F11` is §1.12's `console screen` — a shipped way out of a
/// console filling the display. A module that swallowed either could leave a person with no
/// keyboard route back.
///
/// ⚠️ **The set is closed at two on purpose, because reserving a key costs every module that
/// key for ever.** `reserving_a_key_is_a_deliberate_act` pins the membership so widening it
/// cannot happen quietly — a diff that adds a third has to change a test that says why the
/// other two are there, which is the argument arriving at the moment it is owed.
pub const RESERVED: &[Key] = &[Key::Escape, Key::F11];

/// The pointer buttons. Five, matching what every desktop mouse actually reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum MouseButton {
    Left = 1,
    Middle = 2,
    Right = 3,
    Back = 4,
    Forward = 5,
}

impl MouseButton {
    pub const ALL: &'static [MouseButton] = &[
        MouseButton::Left,
        MouseButton::Middle,
        MouseButton::Right,
        MouseButton::Back,
        MouseButton::Forward,
    ];

    pub const fn to_wire(self) -> u16 {
        self as u16
    }

    pub fn from_wire(v: u16) -> Option<MouseButton> {
        match v {
            1 => Some(MouseButton::Left),
            2 => Some(MouseButton::Middle),
            3 => Some(MouseButton::Right),
            4 => Some(MouseButton::Back),
            5 => Some(MouseButton::Forward),
            _ => None,
        }
    }
}

/// Something that goes down and comes back up. Ascent's `Source`, as this protocol spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Key(Key),
    Mouse(MouseButton),
}

/// The four events, and only the four.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    Down(Button),
    Up(Button),
    /// Relative motion. **Never an absolute position** — see the module docs.
    Pointer { dx: f32, dy: f32 },
    /// Everything held is released. The console sends this on focus loss, on unlatching, and
    /// as the recovery from a ring overflow.
    ReleaseAll,
}

impl InputEvent {
    /// 🚨 May this event be put on the wire at all?
    ///
    /// The one structural guarantee in this file: a [`RESERVED`] key cannot be delivered,
    /// because [`push`] asks this first and there is no other way into the ring. §5.3 asks
    /// for a key the module is *told* it will never receive; this is what makes the telling
    /// true rather than a promise the console has to remember to keep.
    ///
    /// ⚠️ A non-finite pointer delta is refused for a duller reason and it is worth naming:
    /// a `NaN` reaching a producer's camera integrator poisons the transform for the rest of
    /// the session, and the symptom — a view that has vanished to nowhere — looks nothing
    /// like an input bug.
    pub fn is_deliverable(&self) -> bool {
        match self {
            InputEvent::Down(Button::Key(k)) | InputEvent::Up(Button::Key(k)) => {
                !RESERVED.contains(k)
            }
            InputEvent::Pointer { dx, dy } => dx.is_finite() && dy.is_finite(),
            _ => true,
        }
    }

    fn encode(&self, into: &mut [u8]) {
        into.fill(0);
        let (kind, class, code, dx, dy) = match *self {
            InputEvent::Down(b) => {
                let (c, k) = split(b);
                (1u8, c, k, 0.0, 0.0)
            }
            InputEvent::Up(b) => {
                let (c, k) = split(b);
                (2u8, c, k, 0.0, 0.0)
            }
            InputEvent::Pointer { dx, dy } => (3u8, 0u8, 0u16, dx, dy),
            InputEvent::ReleaseAll => (4u8, 0u8, 0u16, 0.0, 0.0),
        };
        into[0] = kind;
        into[1] = class;
        into[2..4].copy_from_slice(&code.to_le_bytes());
        into[4..8].copy_from_slice(&dx.to_le_bytes());
        into[8..12].copy_from_slice(&dy.to_le_bytes());
    }

    /// `None` for anything this build does not recognise.
    ///
    /// ⚠️ **An unknown record is dropped, not guessed at.** A newer console sending a verb an
    /// older module has never heard of must produce *nothing*, never the nearest thing —
    /// "the nearest thing" to an unknown input event is an input event nobody asked for.
    fn decode(src: &[u8]) -> Option<InputEvent> {
        let kind = src[0];
        let class = src[1];
        let code = u16::from_le_bytes(src[2..4].try_into().ok()?);
        let dx = f32::from_le_bytes(src[4..8].try_into().ok()?);
        let dy = f32::from_le_bytes(src[8..12].try_into().ok()?);
        let button = || match class {
            0 => Key::from_wire(code).map(Button::Key),
            1 => MouseButton::from_wire(code).map(Button::Mouse),
            _ => None,
        };
        match kind {
            1 => button().map(InputEvent::Down),
            2 => button().map(InputEvent::Up),
            3 => (dx.is_finite() && dy.is_finite()).then_some(InputEvent::Pointer { dx, dy }),
            4 => Some(InputEvent::ReleaseAll),
            _ => None,
        }
    }
}

fn split(b: Button) -> (u8, u16) {
    match b {
        Button::Key(k) => (0, k.to_wire()),
        Button::Mouse(m) => (1, m.to_wire()),
    }
}

/// Why an event did not reach the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushFault {
    /// [`InputEvent::is_deliverable`] said no. Not an error — the console asked to send
    /// something the protocol does not carry, and it was not carried.
    Refused,
    /// The ring is full, which means the producer has stopped draining it. The overflow flag
    /// is now set and the producer will resynchronise; see [`drain`].
    Full,
}

fn record_off(header: &Header, index: u32) -> u64 {
    header.input_off
        + input_ring::HEADER_BYTES as u64
        + (index % header.input_capacity) as u64 * input_ring::RECORD_BYTES as u64
}

/// Console side: put one event in the ring.
pub(crate) fn push(map: &Mapping, header: &Header, ev: InputEvent) -> Result<(), PushFault> {
    if !ev.is_deliverable() {
        return Err(PushFault::Refused);
    }
    let head_cell = map.u32_at(header.input_off + input_ring::HEAD as u64);
    let tail_cell = map.u32_at(header.input_off + input_ring::TAIL as u64);
    let head = head_cell.load(Ordering::Relaxed);
    let tail = tail_cell.load(Ordering::Acquire);
    if head.wrapping_sub(tail) >= header.input_capacity {
        map.u32_at(header.input_off + input_ring::OVERFLOW as u64).store(1, Ordering::Release);
        return Err(PushFault::Full);
    }
    // SAFETY: the console is the sole writer of the record it is about to claim — `head` has
    // not been published yet, so the producer will not read it.
    let rec = unsafe { map.bytes_mut(record_off(header, head), input_ring::RECORD_BYTES) };
    ev.encode(rec);
    // Release: the record's bytes must be visible before the index that advertises them.
    head_cell.store(head.wrapping_add(1), Ordering::Release);
    Ok(())
}

/// What a drain did, beyond the events themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrainReport {
    /// How many events were handed over, including a synthesised [`InputEvent::ReleaseAll`].
    pub events: usize,
    /// 🚨 The ring had overflowed, so **everything buffered was discarded and one
    /// `ReleaseAll` was delivered instead.** See [`drain`] for why that is the recovery.
    pub recovered_from_overflow: bool,
    /// Records that did not decode — a newer console's verb, or corruption. Counted rather
    /// than guessed at.
    pub undecodable: usize,
}

/// Producer side: take everything waiting.
///
/// 🚨 **On overflow the buffered events are thrown away and a single `ReleaseAll` is
/// delivered in their place, and that is the interesting decision in this file.** A full ring
/// means the console could not push for a while, so the sequence in the ring is a *prefix*
/// with a hole after it — and a prefix can easily contain a `Down` whose `Up` fell in the
/// hole. Replaying it is how a key ends up held for ever inside a module that has no way to
/// learn otherwise, which presents as a ship flying into a wall on its own.
///
/// Discarding costs a fraction of a second of real input. Replaying costs a stuck key with no
/// recovery. `ReleaseAll` exists precisely because "I do not know what you are holding" is a
/// state this protocol has to be able to express.
pub(crate) fn drain(map: &Mapping, header: &Header, out: &mut Vec<InputEvent>) -> DrainReport {
    let head_cell = map.u32_at(header.input_off + input_ring::HEAD as u64);
    let tail_cell = map.u32_at(header.input_off + input_ring::TAIL as u64);
    let overflow_cell = map.u32_at(header.input_off + input_ring::OVERFLOW as u64);

    let head = head_cell.load(Ordering::Acquire);
    let mut report = DrainReport::default();

    if overflow_cell.load(Ordering::Acquire) != 0 {
        tail_cell.store(head, Ordering::Release);
        overflow_cell.store(0, Ordering::Release);
        out.push(InputEvent::ReleaseAll);
        return DrainReport { events: 1, recovered_from_overflow: true, undecodable: 0 };
    }

    let mut tail = tail_cell.load(Ordering::Relaxed);
    while tail != head {
        let rec = map.bytes(record_off(header, tail), input_ring::RECORD_BYTES);
        match InputEvent::decode(rec) {
            Some(ev) => {
                out.push(ev);
                report.events += 1;
            }
            None => report.undecodable += 1,
        }
        tail = tail.wrapping_add(1);
    }
    tail_cell.store(tail, Ordering::Release);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_and_button_round_trips_through_its_wire_code() {
        for &k in Key::ALL {
            assert_eq!(Key::from_wire(k.to_wire()), Some(k), "{k:?}");
        }
        for &m in MouseButton::ALL {
            assert_eq!(MouseButton::from_wire(m.to_wire()), Some(m), "{m:?}");
        }
        assert_eq!(Key::from_wire(0), None);
        assert_eq!(Key::from_wire(0xFFFF), None);
        assert_eq!(MouseButton::from_wire(0), None);
    }

    /// A duplicated HID code would make two keys indistinguishable on the wire, and the
    /// symptom would be one of them arriving as the other.
    #[test]
    fn no_two_keys_share_a_code() {
        let mut seen = std::collections::BTreeSet::new();
        for &k in Key::ALL {
            assert!(seen.insert(k.to_wire()), "{k:?} duplicates code {:#x}", k.to_wire());
        }
    }

    #[test]
    fn every_event_round_trips_through_sixteen_bytes() {
        let cases = [
            InputEvent::Down(Button::Key(Key::W)),
            InputEvent::Up(Button::Key(Key::SuperRight)),
            InputEvent::Down(Button::Mouse(MouseButton::Forward)),
            InputEvent::Up(Button::Mouse(MouseButton::Left)),
            InputEvent::Pointer { dx: -12.5, dy: 0.25 },
            InputEvent::ReleaseAll,
        ];
        let mut buf = [0u8; input_ring::RECORD_BYTES];
        for ev in cases {
            ev.encode(&mut buf);
            assert_eq!(InputEvent::decode(&buf), Some(ev), "{ev:?}");
        }
    }

    #[test]
    fn an_unknown_record_decodes_to_nothing_rather_than_to_the_nearest_thing() {
        let mut buf = [0u8; input_ring::RECORD_BYTES];
        buf[0] = 99; // a verb from a newer console
        assert_eq!(InputEvent::decode(&buf), None);
        buf[0] = 1;
        buf[1] = 7; // a button class nobody defines
        assert_eq!(InputEvent::decode(&buf), None);
        buf[1] = 0;
        buf[2..4].copy_from_slice(&0xFFFFu16.to_le_bytes()); // a key nobody defines
        assert_eq!(InputEvent::decode(&buf), None);
    }

    /// 🚨 §5.3's promise, at the one place it can be kept.
    #[test]
    fn no_reserved_key_can_be_put_on_the_wire() {
        for &k in RESERVED {
            assert!(!InputEvent::Down(Button::Key(k)).is_deliverable(), "{k:?} down");
            assert!(!InputEvent::Up(Button::Key(k)).is_deliverable(), "{k:?} up");
        }
        assert!(InputEvent::Down(Button::Key(Key::W)).is_deliverable());
        // A near neighbour, so the refusal is the set rather than the whole function-key row.
        assert!(InputEvent::Down(Button::Key(Key::F10)).is_deliverable());
        assert!(InputEvent::Down(Button::Key(Key::F12)).is_deliverable());
    }

    /// 🚨 **A change detector, and here that is the point rather than a smell.**
    ///
    /// This is a **permission set**: every entry costs every module that key for ever, and the
    /// standing rule is that it grows by argument and never by tidiness. A test that merely
    /// checked "the reserved keys are refused" would pass just as happily on a set that had
    /// quietly doubled. Pinning the membership means a diff adding a third has to come here and
    /// change the sentence explaining why the other two are members — which is the argument
    /// arriving at the moment it is owed rather than a year later.
    ///
    /// The membership rule, in one line: **a way out of a state a module could otherwise trap
    /// somebody in.** `Escape` leaves the interaction latch; `F11` is `console screen`, the
    /// shipped way out of a console filling the display. "The console also uses it" is not
    /// sufficient — `Ctrl`, `Tab` and every letter would qualify under that.
    #[test]
    fn reserving_a_key_is_a_deliberate_act() {
        assert_eq!(
            RESERVED,
            &[Key::Escape, Key::F11],
            "the reserved set changed; a key here costs every module that key for ever, so the \
             doc comment above this test has to say why the new one is a way OUT rather than \
             merely a key the console uses"
        );
    }

    #[test]
    fn a_non_finite_pointer_delta_never_reaches_a_camera() {
        assert!(!InputEvent::Pointer { dx: f32::NAN, dy: 0.0 }.is_deliverable());
        assert!(!InputEvent::Pointer { dx: 0.0, dy: f32::INFINITY }.is_deliverable());
        assert!(InputEvent::Pointer { dx: 0.0, dy: -0.0 }.is_deliverable());
        // And it cannot sneak in through decode either, which is the path a hostile or
        // mismatched console would take.
        let mut buf = [0u8; input_ring::RECORD_BYTES];
        buf[0] = 3;
        buf[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(InputEvent::decode(&buf), None);
    }

    /// The refusal list is what the protocol carries, so it is asserted rather than described:
    /// four events, and any fifth is a grant that has to arrive with an argument.
    #[test]
    fn the_protocol_carries_four_verbs() {
        let all = [
            InputEvent::Down(Button::Key(Key::A)),
            InputEvent::Up(Button::Key(Key::A)),
            InputEvent::Pointer { dx: 1.0, dy: 1.0 },
            InputEvent::ReleaseAll,
        ];
        let mut buf = [0u8; input_ring::RECORD_BYTES];
        let mut kinds = std::collections::BTreeSet::new();
        for ev in all {
            ev.encode(&mut buf);
            kinds.insert(buf[0]);
        }
        assert_eq!(kinds, [1u8, 2, 3, 4].into_iter().collect());
    }
}
