//! **baseview → egui input translation** — the winit-free half of the UI layer.
//!
//! # Why this exists
//!
//! `ui_layer.rs` drives egui through `egui-winit`, and `egui-winit` needs a `winit::Window`.
//! That is fine in the visual binary, which owns one. It is impossible on the plugin path and
//! in Organon Mind's route-C editor, where the window is an `NSView` nih-plug hands us and
//! there is no winit anywhere in the process. `world.rs` says as much:
//!
//! > **The one coupling left is deliberate**: `FrameTarget::ui_window` is still a
//! > `&winit::Window`, because `ui_layer`'s egui input translation is `egui-winit`.
//!
//! This module is the replacement for that translation, against baseview — the windowing
//! layer nih-plug's editor already runs on. It is **only** input: no window opening, no
//! renderer, no event loop. Those belong to whoever hosts it.
//!
//! # The contract
//!
//! [`State`] mirrors `egui_winit::State`, one call for one call, because the call site it has
//! to serve already exists and works:
//!
//! | `ui_layer.rs` calls | here |
//! |---|---|
//! | `State::new(ctx, ViewportId::ROOT, window, …)` | [`State::new`] |
//! | `state.on_window_event(window, event)` | [`State::on_window_event`] |
//! | `state.take_egui_input(window)` | [`State::take_egui_input`] |
//! | `state.handle_platform_output(window, out)` | [`State::handle_platform_output`] |
//!
//! Two signatures differ, and both differences are forced by baseview rather than chosen:
//!
//! - **the window argument is a [`baseview::WindowInfo`], not a window.** baseview's `Window`
//!   exposes no size and no scale factor — it is a handle you *act on*, not one you *ask*.
//!   Geometry arrives only inside `WindowEvent::Resized(WindowInfo)`, so the host keeps the
//!   latest one (it needs it for the swapchain regardless) and hands it in. That keeps the
//!   egui-winit property that matters: screen rect and scale are read from the window at
//!   `take_egui_input` time, not accumulated from events that may have been missed.
//! - **[`State::handle_platform_output`] takes `&mut baseview::Window`,** because that is the
//!   one call in the set with side effects, and `set_mouse_cursor` needs a real window.
//!
//! # What was corrected against the reference
//!
//! `egui-baseview` (rev `11f487f`, `src/window.rs` + `src/translate.rs`) is the reference this
//! was written from, not copied from. Eight things are deliberately different, each because
//! the reference is wrong or stale for our platform, and each pinned by a test below:
//!
//! 1. **`command` is Meta on macOS**, not Control. The reference's `update_modifiers` sets
//!    `command` from `CONTROL` on every platform and never sets `mac_cmd` or `ctrl` at all —
//!    so on the Mac, ⌘C reaches egui as a bare `C` and ⌃C reaches it as a *copy*. Organon
//!    ships on the Mac. See [`modifiers_for`].
//! 2. **Modifiers come from one source.** The reference derives them twice — from
//!    `keyboard_types::Modifiers` on mouse events and from tracking `ShiftLeft`/`ControlLeft`/…
//!    press-and-release on key events — and the two disagree the moment one is missed (a
//!    key-up delivered while the window is unfocused strands a modifier "held" forever). Here
//!    every event carries `keyboard_types::Modifiers` and that is the only authority, with the
//!    event's own key folded in to fix the platform's before-or-after ambiguity
//!    ([`modifiers_from_key_event`]).
//! 3. **Shifted letters produce a key event.** The reference's character table is lowercase
//!    only, so `Shift`+`Z` matched nothing and ⇧⌘Z (redo) was dead. [`logical_key`] goes
//!    through `egui::Key::from_name`, which knows both cases.
//! 4. **`physical_key` is filled in — and is the fallback.** The reference passes `None`,
//!    which silently breaks egui's shortcut matching on non-QWERTY layouts, the whole reason
//!    egui has the field. The key egui is *told* about is then logical-or-physical
//!    (`egui-winit`'s fallback for egui#3653), so ⌘C works on a Cyrillic layout, where the
//!    logical key is `с` and no Latin `Key` exists at all. See [`physical_key`].
//! 5. **A clipboard shortcut is a clipboard event, not *also* a key event.** The reference
//!    emits `Key { C, command }` alongside `Copy`, so a user shortcut bound to ⌘C fires as
//!    well as the copy. `egui-winit` returns after the clipboard event; so does this. Pasted
//!    text is CRLF-normalised and an empty clipboard produces nothing, for the same reason.
//! 6. **No macOS horizontal-scroll flip.** The reference negates `delta.x` on macOS, citing a
//!    winit bug — and links the issue that closed it. winit 0.30 passes `scrollingDeltaX`
//!    through unmodified and `egui-winit` 0.33 does not flip; baseview reads the same
//!    `NSEvent` field. Keeping the flip would make the editor scroll sideways *opposite* to
//!    the visual window in the same application.
//! 7. **A precise scroll delta is not divided by the scale factor.** baseview labels the
//!    trackpad's `ScrollDelta::Pixels`, but its only producer is macOS's `scrollingDeltaX/Y`,
//!    which are AppKit **points**. (winit converts them logical→physical, which is why
//!    `egui-winit` divides them back down; baseview skips the round trip, so dividing here
//!    would halve trackpad scrolling on every Retina display.) See [`scroll_delta`].
//! 8. **Drag-and-drop is translated — including the accept.** The reference drops
//!    `DragEntered`/`DragMoved`/`DragDropped` on the floor. `DropData::Files` maps exactly onto
//!    egui's `hovered_files` and `dropped_files`, and dropping a `.gguf` onto the window is the
//!    most obvious gesture Organon Mind has. **The accept is the load-bearing half** and the
//!    first cut of this module did not have it: baseview's `EventStatus::AcceptDrop` is not
//!    advisory but the gate the whole gesture passes through on macOS and Windows, so hover
//!    worked and the drop could never land. See [`EventResponse::accept_drop`], which is also
//!    why this module's `EventResponse` has one field `egui_winit`'s does not.
//!
//! # What this deliberately does not do
//!
//! - **It does not read the clipboard.** Copy works out of the box (`baseview::
//!   copy_to_clipboard` exists on all three platforms and [`State::handle_platform_output`]
//!   calls it); paste needs a *reader*, which baseview has no API for. Rather than pick
//!   `copypasta` or `arboard` on the host's behalf, [`State::set_clipboard`] takes a
//!   [`ClipboardText`] provider. With none set, ⌘V is inert — deliberately, and visibly.
//! - **It does not open URLs.** [`PlatformActions::open_url`] reports the request and the
//!   caller decides; `native/Cargo.toml` records that the interface opens no URLs, and this
//!   module is not the place to reverse that.
//! - **It does not touch the cursor on macOS.** `baseview::Window::set_mouse_cursor` is
//!   `todo!()` in the macOS backend at rev `237d323c` — calling it does not degrade, it
//!   **panics**, inside Ableton's process. [`State::handle_platform_output`] is compiled out
//!   there; [`PlatformActions::cursor`] still reports what was wanted, so the host may act.
//!
//! # Testing
//!
//! Every table here is a pure function over types that need no window, which is the point of
//! building this piece in a cloud session: the mapping — including **both** platforms'
//! modifier behaviour, via the `mac` parameter the `cfg!` wrappers pass — is fully covered
//! offline. What is not covered is whether baseview delivers the events we think it does;
//! that is a Mac check.
//!
//! ⚠️ **The blind spot that has already bitten once, stated so it is not rediscovered.** These
//! tests hand a `baseview::MouseEvent` *directly* to [`State::on_window_event`], which means
//! they sit **downstream of whatever the platform decides to emit**. Where a backend only
//! produces an event conditionally, a test here can pass against a sequence the real platform
//! would never deliver. Drag-and-drop was exactly that: the drop is gated on the value returned
//! for the *hover*, and X11 emits no drag events whatsoever at rev `237d323c`, so on Linux the
//! entire path is unreachable dead code that tests green. Anything with a **return-value
//! contract back to the platform** — as opposed to a pure input mapping — has to be read out of
//! the backend source, and confirmed on a machine that runs it.

use egui::{pos2, vec2, Pos2, Rect};
use keyboard_types::{Code, Key as KbKey, KeyState, KeyboardEvent, Modifiers as KbModifiers};

// ═════════════════════════════════════════════════════════════════════════════
// Mapping tables — pure, and the whole reason this file can be built here
// ═════════════════════════════════════════════════════════════════════════════

/// baseview mouse button → egui pointer button.
///
/// `Other(_)` has no egui equivalent and is dropped; `Back`/`Forward` are egui's `Extra1`/
/// `Extra2`, which the reference throws away along with `Other`.
pub fn pointer_button(button: baseview::MouseButton) -> Option<egui::PointerButton> {
    Some(match button {
        baseview::MouseButton::Left => egui::PointerButton::Primary,
        baseview::MouseButton::Right => egui::PointerButton::Secondary,
        baseview::MouseButton::Middle => egui::PointerButton::Middle,
        baseview::MouseButton::Back => egui::PointerButton::Extra1,
        baseview::MouseButton::Forward => egui::PointerButton::Extra2,
        baseview::MouseButton::Other(_) => return None,
    })
}

/// The **logical** key — what the layout says the key means — as egui understands it.
///
/// Named keys are mapped by hand; printable keys arrive as `Key::Character` and are handed to
/// `egui::Key::from_name`, which is egui's own single-character parser and therefore cannot
/// drift from egui's key set. Restricted to one `char` on purpose: a `Character` payload is
/// the *typed text*, and `from_name` would otherwise read a pasted "Copy" as `Key::Copy`.
pub fn logical_key(key: &KbKey) -> Option<egui::Key> {
    use egui::Key;

    Some(match key {
        KbKey::ArrowDown => Key::ArrowDown,
        KbKey::ArrowLeft => Key::ArrowLeft,
        KbKey::ArrowRight => Key::ArrowRight,
        KbKey::ArrowUp => Key::ArrowUp,

        KbKey::Escape => Key::Escape,
        KbKey::Tab => Key::Tab,
        KbKey::Backspace => Key::Backspace,
        KbKey::Enter => Key::Enter,

        KbKey::Insert => Key::Insert,
        KbKey::Delete => Key::Delete,
        KbKey::Home => Key::Home,
        KbKey::End => Key::End,
        KbKey::PageUp => Key::PageUp,
        KbKey::PageDown => Key::PageDown,

        // egui has first-class Copy/Cut/Paste keys (real keyboards have them, and macOS
        // synthesises them). Mapping them here is what makes `is_*_command` below a
        // *fallback* rather than the only path.
        KbKey::Copy => Key::Copy,
        KbKey::Cut => Key::Cut,
        KbKey::Paste => Key::Paste,

        KbKey::BrowserBack => Key::BrowserBack,

        KbKey::F1 => Key::F1,
        KbKey::F2 => Key::F2,
        KbKey::F3 => Key::F3,
        KbKey::F4 => Key::F4,
        KbKey::F5 => Key::F5,
        KbKey::F6 => Key::F6,
        KbKey::F7 => Key::F7,
        KbKey::F8 => Key::F8,
        KbKey::F9 => Key::F9,
        KbKey::F10 => Key::F10,
        KbKey::F11 => Key::F11,
        KbKey::F12 => Key::F12,
        KbKey::F13 => Key::F13,
        KbKey::F14 => Key::F14,
        KbKey::F15 => Key::F15,
        KbKey::F16 => Key::F16,
        KbKey::F17 => Key::F17,
        KbKey::F18 => Key::F18,
        KbKey::F19 => Key::F19,
        KbKey::F20 => Key::F20,
        KbKey::F21 => Key::F21,
        KbKey::F22 => Key::F22,
        KbKey::F23 => Key::F23,
        KbKey::F24 => Key::F24,

        KbKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                // A multi-char payload is composed text (a dead key resolving, an IME
                // commit). It has no single key identity; the `Text` event carries it.
                return None;
            }
            let mut buf = [0u8; 4];
            return Key::from_name(c.encode_utf8(&mut buf));
        }

        _ => return None,
    })
}

/// The **physical** key — where the key sits on the board, layout-independent.
///
/// egui uses this to match shortcuts for people who are not on QWERTY: ⌘Z has to be the key in
/// the bottom-left corner whether the layout calls it `z` (QWERTY) or `w` (AZERTY). The
/// reference passes `None` here, which is why that silently did not work.
///
/// Numpad digits fold onto `Num0`..`Num9`, which is what egui's own doc comment on those
/// variants specifies ("from main row or numpad"). Keys egui has no name for — `NumpadStar`,
/// `IntlYen`, the media block — return `None` rather than being forced onto a near-miss.
pub fn physical_key(code: Code) -> Option<egui::Key> {
    use egui::Key;

    Some(match code {
        // Punctuation, in `Code`'s own order.
        Code::Backquote => Key::Backtick,
        Code::Backslash => Key::Backslash,
        Code::BracketLeft => Key::OpenBracket,
        Code::BracketRight => Key::CloseBracket,
        Code::Comma => Key::Comma,
        Code::Equal => Key::Equals,
        Code::Minus => Key::Minus,
        Code::Period => Key::Period,
        Code::Quote => Key::Quote,
        Code::Semicolon => Key::Semicolon,
        Code::Slash => Key::Slash,

        Code::Digit0 => Key::Num0,
        Code::Digit1 => Key::Num1,
        Code::Digit2 => Key::Num2,
        Code::Digit3 => Key::Num3,
        Code::Digit4 => Key::Num4,
        Code::Digit5 => Key::Num5,
        Code::Digit6 => Key::Num6,
        Code::Digit7 => Key::Num7,
        Code::Digit8 => Key::Num8,
        Code::Digit9 => Key::Num9,

        Code::KeyA => Key::A,
        Code::KeyB => Key::B,
        Code::KeyC => Key::C,
        Code::KeyD => Key::D,
        Code::KeyE => Key::E,
        Code::KeyF => Key::F,
        Code::KeyG => Key::G,
        Code::KeyH => Key::H,
        Code::KeyI => Key::I,
        Code::KeyJ => Key::J,
        Code::KeyK => Key::K,
        Code::KeyL => Key::L,
        Code::KeyM => Key::M,
        Code::KeyN => Key::N,
        Code::KeyO => Key::O,
        Code::KeyP => Key::P,
        Code::KeyQ => Key::Q,
        Code::KeyR => Key::R,
        Code::KeyS => Key::S,
        Code::KeyT => Key::T,
        Code::KeyU => Key::U,
        Code::KeyV => Key::V,
        Code::KeyW => Key::W,
        Code::KeyX => Key::X,
        Code::KeyY => Key::Y,
        Code::KeyZ => Key::Z,

        Code::Backspace => Key::Backspace,
        Code::Enter => Key::Enter,
        Code::Space => Key::Space,
        Code::Tab => Key::Tab,
        Code::Escape => Key::Escape,

        Code::Delete => Key::Delete,
        Code::End => Key::End,
        // egui folds Help onto Insert (`Key::from_name`: `"Help" | "Insert"`), which is the
        // old Mac keyboard where the Insert key is labelled Help.
        Code::Help => Key::Insert,
        Code::Home => Key::Home,
        Code::Insert => Key::Insert,
        Code::PageDown => Key::PageDown,
        Code::PageUp => Key::PageUp,

        Code::ArrowDown => Key::ArrowDown,
        Code::ArrowLeft => Key::ArrowLeft,
        Code::ArrowRight => Key::ArrowRight,
        Code::ArrowUp => Key::ArrowUp,

        Code::Numpad0 => Key::Num0,
        Code::Numpad1 => Key::Num1,
        Code::Numpad2 => Key::Num2,
        Code::Numpad3 => Key::Num3,
        Code::Numpad4 => Key::Num4,
        Code::Numpad5 => Key::Num5,
        Code::Numpad6 => Key::Num6,
        Code::Numpad7 => Key::Num7,
        Code::Numpad8 => Key::Num8,
        Code::Numpad9 => Key::Num9,
        Code::NumpadAdd => Key::Plus,
        Code::NumpadBackspace => Key::Backspace,
        Code::NumpadComma => Key::Comma,
        Code::NumpadDecimal => Key::Period,
        Code::NumpadDivide => Key::Slash,
        Code::NumpadEnter => Key::Enter,
        Code::NumpadEqual => Key::Equals,
        Code::NumpadSubtract => Key::Minus,

        Code::F1 => Key::F1,
        Code::F2 => Key::F2,
        Code::F3 => Key::F3,
        Code::F4 => Key::F4,
        Code::F5 => Key::F5,
        Code::F6 => Key::F6,
        Code::F7 => Key::F7,
        Code::F8 => Key::F8,
        Code::F9 => Key::F9,
        Code::F10 => Key::F10,
        Code::F11 => Key::F11,
        Code::F12 => Key::F12,
        Code::F13 => Key::F13,
        Code::F14 => Key::F14,
        Code::F15 => Key::F15,
        Code::F16 => Key::F16,
        Code::F17 => Key::F17,
        Code::F18 => Key::F18,
        Code::F19 => Key::F19,
        Code::F20 => Key::F20,
        Code::F21 => Key::F21,
        Code::F22 => Key::F22,
        Code::F23 => Key::F23,
        Code::F24 => Key::F24,

        Code::BrowserBack => Key::BrowserBack,
        Code::Copy => Key::Copy,
        Code::Cut => Key::Cut,
        Code::Paste => Key::Paste,

        _ => return None,
    })
}

/// `keyboard_types` modifier bits → egui modifiers, for a stated platform.
///
/// **The `mac` parameter is the point of this function.** egui's `Modifiers` distinguishes
/// three things the reference collapses into one:
///
/// | egui field | means | macOS | elsewhere |
/// |---|---|---|---|
/// | `ctrl` | the physical Control key | Control | Control |
/// | `mac_cmd` | the physical Command key | Meta | never |
/// | `command` | *the shortcut modifier* | Meta | Control |
///
/// `command` is what every egui shortcut is actually written against, so getting it wrong does
/// not produce a subtly wrong shortcut — it produces a keyboard where ⌘C does nothing and ⌃C
/// copies. Taking the platform as an argument is what lets both columns be tested from a
/// Linux cloud session; [`modifiers`] is the `cfg!` wrapper that picks the real one.
pub fn modifiers_for(m: KbModifiers, mac: bool) -> egui::Modifiers {
    let ctrl = m.contains(KbModifiers::CONTROL);
    let meta = m.contains(KbModifiers::META);
    egui::Modifiers {
        alt: m.contains(KbModifiers::ALT),
        ctrl,
        shift: m.contains(KbModifiers::SHIFT),
        mac_cmd: mac && meta,
        command: if mac { meta } else { ctrl },
    }
}

/// [`modifiers_for`] against the platform this build targets.
pub fn modifiers(m: KbModifiers) -> egui::Modifiers {
    modifiers_for(m, cfg!(target_os = "macos"))
}

/// Modifier state for a keyboard event, with the event's own key folded in.
///
/// Platforms disagree about whether the modifier mask on a key event is the state *before* or
/// *after* that key — X11's `state` field is famously the mask before the event, so pressing
/// Shift arrives with `SHIFT` clear and releasing it arrives with `SHIFT` set. Reading the mask
/// alone therefore reports every modifier one keystroke late.
///
/// The fix is not to track modifiers separately (that is the reference's approach, and it
/// strands a modifier "held" whenever a key-up is lost to a focus change). It is to take the
/// mask as the baseline and let the event override its own bit: a `Shift` down *is* shift down,
/// whatever the mask says.
pub fn modifiers_from_key_event(event: &KeyboardEvent, mac: bool) -> egui::Modifiers {
    let mut out = modifiers_for(event.modifiers, mac);
    let down = event.state == KeyState::Down;

    // Match on the physical code, not the logical key: a remapped Caps-Lock-as-Control still
    // reports `Code::ControlLeft`, and `Key::Control` is what an unmapped one reports. Both
    // are checked so neither platform convention is missed.
    match event.code {
        Code::ShiftLeft | Code::ShiftRight => out.shift = down,
        Code::ControlLeft | Code::ControlRight => {
            out.ctrl = down;
            if !mac {
                out.command = down;
            }
        }
        Code::AltLeft | Code::AltRight => out.alt = down,
        Code::MetaLeft | Code::MetaRight => {
            if mac {
                out.mac_cmd = down;
                out.command = down;
            }
        }
        _ => match &event.key {
            KbKey::Shift => out.shift = down,
            KbKey::Control => {
                out.ctrl = down;
                if !mac {
                    out.command = down;
                }
            }
            KbKey::Alt => out.alt = down,
            KbKey::Meta => {
                if mac {
                    out.mac_cmd = down;
                    out.command = down;
                }
            }
            _ => {}
        },
    }
    out
}

/// A scroll delta, in the unit and magnitude egui expects.
///
/// `scale` converts a baseview **logical** quantity into an **egui point** — see
/// [`State::baseview_to_points`]. It is `1.0` in every ordinary configuration and only moves
/// when the caller forces a `native_pixels_per_point` that disagrees with the window's own
/// scale factor.
///
/// The `Pixels` variant is *not* divided by the scale factor, which is the single most
/// surprising line in this file. baseview only ever produces it on macOS, from
/// `NSEvent::scrollingDeltaX/Y` with `hasPreciseScrollingDeltas` — and those are AppKit
/// **points**, already logical. winit converts them logical→physical, which is why
/// `egui-winit` divides them back down again; baseview hands them over unconverted, so the
/// same division here would halve trackpad scrolling on every Retina display.
pub fn scroll_delta(delta: baseview::ScrollDelta, scale: f32) -> (egui::MouseWheelUnit, egui::Vec2) {
    match delta {
        baseview::ScrollDelta::Lines { x, y } => (egui::MouseWheelUnit::Line, vec2(x, y)),
        baseview::ScrollDelta::Pixels { x, y } => (egui::MouseWheelUnit::Point, vec2(x, y) * scale),
    }
}

/// egui cursor → baseview cursor.
///
/// Exhaustive on purpose — no `_` arm — so a new `egui::CursorIcon` is a compile error here
/// rather than a cursor that silently reverts to an arrow.
pub fn cursor_icon(cursor: egui::CursorIcon) -> baseview::MouseCursor {
    use baseview::MouseCursor as C;
    use egui::CursorIcon as E;
    match cursor {
        E::Default => C::Default,
        E::None => C::Hidden,
        E::ContextMenu => C::Hand,
        E::Help => C::Help,
        E::PointingHand => C::Hand,
        E::Progress => C::PtrWorking,
        E::Wait => C::Working,
        E::Cell => C::Cell,
        E::Crosshair => C::Crosshair,
        E::Text => C::Text,
        E::VerticalText => C::VerticalText,
        E::Alias => C::Alias,
        E::Copy => C::Copy,
        E::Move => C::Move,
        // `NoDrop` is "a drag is in flight and *this spot* will not take it"; `NotAllowed` is
        // "this control is disabled". baseview has both cursors; the reference collapses them.
        E::NoDrop => C::PtrNotAllowed,
        E::NotAllowed => C::NotAllowed,
        E::Grab => C::Hand,
        E::Grabbing => C::HandGrabbing,
        E::AllScroll => C::AllScroll,
        E::ResizeHorizontal => C::EwResize,
        E::ResizeNeSw => C::NeswResize,
        E::ResizeNwSe => C::NwseResize,
        E::ResizeVertical => C::NsResize,
        E::ResizeEast => C::EResize,
        E::ResizeSouthEast => C::SeResize,
        E::ResizeSouth => C::SResize,
        E::ResizeSouthWest => C::SwResize,
        E::ResizeWest => C::WResize,
        E::ResizeNorthWest => C::NwResize,
        E::ResizeNorth => C::NResize,
        E::ResizeNorthEast => C::NeResize,
        E::ResizeColumn => C::ColResize,
        E::ResizeRow => C::RowResize,
        E::ZoomIn => C::ZoomIn,
        E::ZoomOut => C::ZoomOut,
    }
}

/// A clipboard gesture recognised from a keypress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardCommand {
    Cut,
    Copy,
    Paste,
}

impl ClipboardCommand {
    /// Recognise ⌘X / ⌘C / ⌘V (⌃ off the Mac), the dedicated Cut/Copy/Paste keys, and
    /// Windows' Shift+Delete / Ctrl+Insert / Shift+Insert.
    ///
    /// `key` is the **logical** key, so this fires on the character the layout produces rather
    /// than on a physical position — which is what a user pressing ⌘C on AZERTY expects.
    ///
    /// `windows` is a parameter for the same reason `mac` is one on [`modifiers_for`]: so the
    /// other platform's behaviour is reachable from a test here.
    pub fn recognise(
        modifiers: egui::Modifiers,
        key: egui::Key,
        windows: bool,
    ) -> Option<Self> {
        use egui::Key;
        Some(match key {
            Key::Cut => Self::Cut,
            Key::Copy => Self::Copy,
            Key::Paste => Self::Paste,
            Key::X if modifiers.command => Self::Cut,
            Key::C if modifiers.command => Self::Copy,
            Key::V if modifiers.command => Self::Paste,
            Key::Delete if windows && modifiers.shift => Self::Cut,
            Key::Insert if windows && modifiers.ctrl => Self::Copy,
            Key::Insert if windows && modifiers.shift => Self::Paste,
            _ => return None,
        })
    }

    /// [`Self::recognise`] against the platform this build targets.
    pub fn detect(modifiers: egui::Modifiers, key: egui::Key) -> Option<Self> {
        Self::recognise(modifiers, key, cfg!(target_os = "windows"))
    }
}

/// Should this string be delivered to egui as typed text?
///
/// Control characters must not be: a Return that a platform reports as `Character("\r")` would
/// otherwise insert a literal carriage return into a text field *in addition to* the `Enter`
/// key event. Private-use-area code points are the other trap — that is where macOS puts the
/// function keys and the arrows, so an unfiltered Home key types `\u{f729}`.
pub fn is_typed_text(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_printable_char)
}

fn is_printable_char(c: char) -> bool {
    let private_use = ('\u{e000}'..='\u{f8ff}').contains(&c)
        || ('\u{f0000}'..='\u{ffffd}').contains(&c)
        || ('\u{100000}'..='\u{10fffd}').contains(&c);
    !private_use && !c.is_ascii_control()
}

// ═════════════════════════════════════════════════════════════════════════════
// The four calls
// ═════════════════════════════════════════════════════════════════════════════

/// What [`State::on_window_event`] decided about an event.
///
/// `consumed` and `repaint` are `egui_winit::EventResponse`'s two fields, unchanged, so the
/// call site reads identically. [`Self::accept_drop`] is a third one that baseview's platform
/// contract *requires* and winit has no analogue for — see below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventResponse {
    /// egui wants exclusive use of this event — do not also act on it.
    pub consumed: bool,
    /// egui needs a repaint because of this event.
    pub repaint: bool,
    /// **Set only on drag events, and load-bearing: without it no drop ever happens.**
    ///
    /// baseview's `EventStatus` has a third variant, `AcceptDrop(DropEffect)`, and on macOS and
    /// Windows it is not advisory — it is the gate the whole drag-and-drop gesture passes
    /// through. `macos/view.rs`'s `on_event` maps *anything else*, `Captured` and `Ignored`
    /// alike, to `NSDragOperationNone`; `prepare_for_drag_operation` and
    /// `perform_drag_operation` are then never reached ("This function won't be called unless
    /// dragging_entered/updated has returned an acceptable operation"), so `DragDropped` is
    /// never emitted and the file bounces back. `win/drop_target.rs` has the identical gate
    /// onto `DROPEFFECT_NONE`. `perform_drag_operation` *also* requires `AcceptDrop(_)` to
    /// report success, so the drop event has to accept too, not just the hover events.
    ///
    /// This is invisible on X11, which emits no drag events **at all** at rev `237d323c` — the
    /// whole path is unreachable there, which is exactly how a first cut written and tested on
    /// Linux can have working hover and a drop that can never land.
    pub accept_drop: Option<baseview::DropEffect>,
}

impl EventResponse {
    /// The verdict in the form baseview's `WindowHandler::on_event` must return.
    ///
    /// `Ignored` is not "we did nothing" — it hands the event back to the host, which is how a
    /// DAW keeps playing on the space bar while a plugin window has focus. Reporting
    /// `Captured` for everything is how a plugin steals the transport.
    ///
    /// [`Self::accept_drop`] outranks `consumed`, because on a drag event the platform is not
    /// asking "did you handle this" but "will you take this" — and both of the other two
    /// answers mean no.
    pub fn status(self) -> baseview::EventStatus {
        match self.accept_drop {
            Some(effect) => baseview::EventStatus::AcceptDrop(effect),
            None if self.consumed => baseview::EventStatus::Captured,
            None => baseview::EventStatus::Ignored,
        }
    }
}

/// How a drop we accept is reported back to the source application.
///
/// `Copy` is the honest answer for every drop this translates: opening a `.gguf` (or an `.hdr`,
/// or a preset) reads the file and leaves the original exactly where it was. `Move` would tell
/// the source app to delete it.
const DROP_EFFECT: baseview::DropEffect = baseview::DropEffect::Copy;

/// Somewhere to read pasted text from.
///
/// baseview can *write* the clipboard (`baseview::copy_to_clipboard`) but cannot read it, and
/// choosing a reader — `copypasta`, `arboard`, an AppKit call — is the host's decision, not
/// this module's. Without one, [`ClipboardCommand::Paste`] produces no event at all rather
/// than an empty one, so the failure is "nothing happened", never "your text vanished".
pub trait ClipboardText: Send {
    /// The clipboard's current text, or `None` if it holds none / could not be read.
    fn get(&mut self) -> Option<String>;
}

/// What [`State::handle_platform_output`] would do, as data.
///
/// Split out from the doing so the decisions are testable without a window — which matters
/// most for `cursor`, whose *change detection* is the part that has a bug in it if anything
/// does, and for the macOS cursor call that must never actually happen (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlatformActions {
    /// Text egui wants placed on the clipboard.
    pub copy_text: Option<String>,
    /// A URL egui wants opened. **Not opened by this module** — reported only.
    pub open_url: Option<String>,
    /// The cursor to set, `None` when it has not changed since the last frame. baseview's X11
    /// backend also dedupes, but Windows' does not and macOS's panics, so this is ours to own.
    pub cursor: Option<baseview::MouseCursor>,
}

/// egui's per-window input state, driven by baseview instead of winit.
///
/// One per viewport, exactly like `egui_winit::State`.
pub struct State {
    ctx: egui::Context,
    viewport_id: egui::ViewportId,
    start_time: std::time::Instant,
    input: egui::RawInput,
    /// Where egui thinks the pointer is, in points. Button events need it because baseview
    /// reports the button without a position.
    pointer_pos_in_points: Option<Pos2>,
    /// `Some` = the caller pinned the scale; `None` = follow the window's own scale factor.
    native_pixels_per_point: Option<f32>,
    /// The last cursor actually pushed to the window, so an unchanged one is not re-pushed.
    current_cursor_icon: Option<baseview::MouseCursor>,
    clipboard: Option<Box<dyn ClipboardText>>,
}

impl State {
    /// Mirror of `egui_winit::State::new`.
    ///
    /// `window` is the window's geometry at open time — baseview will not volunteer it again
    /// until the first `Resized`, so egui would otherwise lay out its first frame against an
    /// empty screen rect.
    ///
    /// `native_pixels_per_point`: `None` takes the scale factor from the window, which is what
    /// is wanted on a Retina display and on a projector alike — the same choice, and the same
    /// reasoning, as the `egui-winit` call site in `ui_layer.rs`.
    pub fn new(
        ctx: egui::Context,
        viewport_id: egui::ViewportId,
        window: &baseview::WindowInfo,
        native_pixels_per_point: Option<f32>,
        theme: Option<egui::Theme>,
        max_texture_side: Option<usize>,
    ) -> Self {
        let mut slf = Self {
            ctx,
            viewport_id,
            start_time: std::time::Instant::now(),
            input: egui::RawInput {
                // baseview tells us on the first `Focused`/`Unfocused`; assuming focus would
                // make a background window draw a blinking caret.
                focused: false,
                system_theme: theme,
                max_texture_side,
                ..Default::default()
            },
            pointer_pos_in_points: None,
            native_pixels_per_point,
            current_cursor_icon: None,
            clipboard: None,
        };
        slf.input.viewport_id = viewport_id;
        let ppp = slf.resolve_pixels_per_point(window);
        let rect = screen_rect(window, ppp);
        slf.input.screen_rect = rect;
        let vp = slf.input.viewports.entry(viewport_id).or_default();
        vp.native_pixels_per_point = Some(ppp);
        vp.inner_rect = rect;
        slf
    }

    /// Give the state somewhere to read pasted text from. See [`ClipboardText`].
    pub fn set_clipboard(&mut self, clipboard: Box<dyn ClipboardText>) {
        self.clipboard = Some(clipboard);
    }

    /// The egui context this state feeds.
    pub fn egui_ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Points per physical pixel for `window` under the current policy.
    ///
    /// Guarded against a garbage scale factor (0, negative, NaN): a zero here would divide the
    /// screen rect to infinity and egui would lay out a window it can never draw.
    pub fn resolve_pixels_per_point(&self, window: &baseview::WindowInfo) -> f32 {
        let ppp = self.native_pixels_per_point.unwrap_or(window.scale() as f32);
        if ppp.is_finite() && ppp > 0.0 {
            ppp
        } else {
            1.0
        }
    }

    /// Convert a baseview **logical** coordinate into an **egui point**.
    ///
    /// baseview's logical units are physical ÷ `window.scale()`; egui's points are physical ÷
    /// `pixels_per_point`. Those are the same number in every ordinary configuration, and this
    /// ratio is then 1.0. They differ exactly when the caller pinned `native_pixels_per_point`
    /// to something other than the window's own scale — at which point *not* correcting would
    /// put the cursor somewhere the user is not pointing.
    pub fn baseview_to_points(&self, window: &baseview::WindowInfo) -> f32 {
        window.scale() as f32 / self.resolve_pixels_per_point(window)
    }

    /// Mirror of `egui_winit::State::take_egui_input`.
    ///
    /// Stamps the frame's time and re-reads the window's geometry, so a resize that arrived
    /// without an event (or was coalesced away) still reaches egui.
    pub fn take_egui_input(&mut self, window: &baseview::WindowInfo) -> egui::RawInput {
        self.input.time = Some(self.start_time.elapsed().as_secs_f64());

        let ppp = self.resolve_pixels_per_point(window);
        let rect = screen_rect(window, ppp);
        self.input.screen_rect = rect;
        self.input.viewport_id = self.viewport_id;

        let vp = self.input.viewports.entry(self.viewport_id).or_default();
        vp.native_pixels_per_point = Some(ppp);
        vp.inner_rect = rect;

        self.input.take()
    }

    /// Mirror of `egui_winit::State::on_window_event`.
    ///
    /// `window` is the caller's current [`baseview::WindowInfo`]; it is read for the scale
    /// factor, which pointer and scroll conversion both need.
    pub fn on_window_event(
        &mut self,
        window: &baseview::WindowInfo,
        event: &baseview::Event,
    ) -> EventResponse {
        match event {
            baseview::Event::Mouse(e) => self.on_mouse_event(window, e),
            baseview::Event::Keyboard(e) => self.on_keyboard_event(e),
            baseview::Event::Window(e) => self.on_window_state_event(e),
        }
    }

    fn on_mouse_event(
        &mut self,
        window: &baseview::WindowInfo,
        event: &baseview::MouseEvent,
    ) -> EventResponse {
        let scale = self.baseview_to_points(window);
        match event {
            baseview::MouseEvent::CursorMoved { position, modifiers: mods } => {
                self.input.modifiers = modifiers(*mods);
                let pos = to_points(*position, scale);
                self.pointer_pos_in_points = Some(pos);
                self.input.events.push(egui::Event::PointerMoved(pos));
                EventResponse { repaint: true, consumed: self.ctx.is_using_pointer(), ..Default::default() }
            }
            baseview::MouseEvent::ButtonPressed { button, modifiers: mods } => {
                self.input.modifiers = modifiers(*mods);
                self.push_pointer_button(*button, true);
                EventResponse { repaint: true, consumed: self.ctx.wants_pointer_input(), ..Default::default() }
            }
            baseview::MouseEvent::ButtonReleased { button, modifiers: mods } => {
                self.input.modifiers = modifiers(*mods);
                self.push_pointer_button(*button, false);
                EventResponse { repaint: true, consumed: self.ctx.wants_pointer_input(), ..Default::default() }
            }
            baseview::MouseEvent::WheelScrolled { delta, modifiers: mods } => {
                self.input.modifiers = modifiers(*mods);
                let (unit, delta) = scroll_delta(*delta, scale);
                let mods = self.input.modifiers;
                self.input.events.push(egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers: mods,
                });
                EventResponse { repaint: true, consumed: self.ctx.wants_pointer_input(), ..Default::default() }
            }
            baseview::MouseEvent::CursorLeft => {
                self.pointer_pos_in_points = None;
                self.input.events.push(egui::Event::PointerGone);
                EventResponse { repaint: true, consumed: false, ..Default::default() }
            }
            // egui has no "pointer entered" event — the next move carries the position, and
            // that is the event it reacts to.
            baseview::MouseEvent::CursorEntered => {
                EventResponse { repaint: true, consumed: false, ..Default::default() }
            }

            // ── drag and drop ───────────────────────────────────────────────
            // `hovered_files` is *state*, not a queue: egui expects the current hover set each
            // frame, so every drag event rewrites it rather than appending. `RawInput::take`
            // **clones** it (while `mem::take`-ing `events` and `dropped_files`), so a hover
            // that spans frames keeps showing even once the pointer stops moving and baseview
            // goes quiet. That asymmetry is egui's, not ours, so it is pinned by test —
            // `a_stationary_hover_survives_frames_with_no_events`.
            baseview::MouseEvent::DragEntered { position, modifiers: mods, data }
            | baseview::MouseEvent::DragMoved { position, modifiers: mods, data } => {
                self.input.modifiers = modifiers(*mods);
                let pos = to_points(*position, scale);
                self.pointer_pos_in_points = Some(pos);
                self.input.events.push(egui::Event::PointerMoved(pos));
                self.input.hovered_files = hovered_files(data);
                // Accepting here is what makes the drop reachable at all: the platform reads
                // this answer to decide whether to keep the gesture alive. See
                // `EventResponse::accept_drop`. Files we cannot name we do not claim.
                let any = !self.input.hovered_files.is_empty();
                EventResponse {
                    repaint: true,
                    consumed: any,
                    accept_drop: any.then_some(DROP_EFFECT),
                }
            }
            baseview::MouseEvent::DragLeft => {
                self.input.hovered_files.clear();
                // Explicitly *not* accepting: the drag has left, so the answer to "will you
                // take this" is a real no rather than an inapplicable question.
                EventResponse { repaint: true, consumed: false, accept_drop: None }
            }
            baseview::MouseEvent::DragDropped { position, modifiers: mods, data } => {
                self.input.modifiers = modifiers(*mods);
                let pos = to_points(*position, scale);
                self.pointer_pos_in_points = Some(pos);
                self.input.events.push(egui::Event::PointerMoved(pos));
                self.input.hovered_files.clear();
                let dropped = dropped_files(data);
                let any = !dropped.is_empty();
                self.input.dropped_files.extend(dropped);
                // The drop must accept too — `perform_drag_operation` returns `YES` only for
                // `AcceptDrop(_)`, and a `NO` there tells the source app the drop failed even
                // though we have already taken the paths.
                EventResponse {
                    repaint: true,
                    consumed: any,
                    accept_drop: any.then_some(DROP_EFFECT),
                }
            }
        }
    }

    fn push_pointer_button(&mut self, button: baseview::MouseButton, pressed: bool) {
        // baseview reports the button without a position, so the last move is the authority.
        // No position yet (a click delivered before any motion) means no event, which is what
        // the reference does and what egui-winit does.
        if let (Some(pos), Some(button)) = (self.pointer_pos_in_points, pointer_button(button)) {
            let modifiers = self.input.modifiers;
            self.input.events.push(egui::Event::PointerButton { pos, button, pressed, modifiers });
        }
    }

    fn on_keyboard_event(&mut self, event: &KeyboardEvent) -> EventResponse {
        let mac = cfg!(target_os = "macos");
        self.input.modifiers = modifiers_from_key_event(event, mac);
        let modifiers = self.input.modifiers;
        let pressed = event.state == KeyState::Down;

        let physical = physical_key(event.code);
        // **Logical OR physical**, in that order — `egui-winit`'s fallback for layouts with no
        // Latin characters (egui#3653). On a Cyrillic layout ⌘C reports the logical key `с`,
        // which egui has no `Key` for; the *physical* key is still `KeyC`, so the shortcut
        // survives. Without this, copy and paste simply do not exist on those layouts.
        let active = logical_key(&event.key).or(physical);

        // A clipboard shortcut is *either* a clipboard event or a key event, never both —
        // also `egui-winit`'s behaviour, and the reason matters: emitting `Key { C, command }`
        // alongside `Copy` fires any user shortcut bound to ⌘C *as well as* copying.
        let clipboard =
            if pressed { active.and_then(|k| ClipboardCommand::detect(modifiers, k)) } else { None };

        match clipboard {
            Some(ClipboardCommand::Cut) => self.input.events.push(egui::Event::Cut),
            Some(ClipboardCommand::Copy) => self.input.events.push(egui::Event::Copy),
            Some(ClipboardCommand::Paste) => {
                if let Some(text) = self.clipboard.as_mut().and_then(|c| c.get()) {
                    // Windows hands back CRLF and egui's text model is LF-only, so pasting a
                    // Windows file into a field would otherwise carry a stray \r on every line.
                    let text = text.replace("\r\n", "\n");
                    // An empty clipboard produces nothing rather than an empty edit, which
                    // would still push undo history and mark the field dirty.
                    if !text.is_empty() {
                        self.input.events.push(egui::Event::Paste(text));
                    }
                }
            }
            None => {
                if let Some(key) = active {
                    self.input.events.push(egui::Event::Key {
                        key,
                        physical_key: physical,
                        // baseview tracks auto-repeat itself, so pass the truth. (`egui-winit`
                        // passes `false` and lets egui infer it, because winit's flag is not
                        // reliable across backends.)
                        repeat: event.repeat,
                        pressed,
                        modifiers,
                    });
                }
                // Text, but only when no shortcut modifier is down: ⌘S must save, not type an
                // "s" into whatever field has focus. Alt is *not* excluded — on macOS Option is
                // how you type ©, ø and the rest.
                let is_cmd = modifiers.ctrl || modifiers.command || modifiers.mac_cmd;
                if pressed && !is_cmd {
                    if let KbKey::Character(s) = &event.key {
                        if is_typed_text(s) {
                            self.input.events.push(egui::Event::Text(s.clone()));
                        }
                    }
                }
            }
        }

        // Tab always consumes: egui uses it to move focus between widgets, so passing it back
        // to the host would move focus *and* do whatever the host does with Tab.
        let consumed = self.ctx.wants_keyboard_input() || active == Some(egui::Key::Tab);
        EventResponse { repaint: true, consumed, ..Default::default() }
    }

    fn on_window_state_event(&mut self, event: &baseview::WindowEvent) -> EventResponse {
        match event {
            baseview::WindowEvent::Resized(info) => {
                let ppp = self.resolve_pixels_per_point(info);
                let rect = screen_rect(info, ppp);
                self.input.screen_rect = rect;
                let vp = self.input.viewports.entry(self.viewport_id).or_default();
                vp.native_pixels_per_point = Some(ppp);
                vp.inner_rect = rect;
                EventResponse { repaint: true, consumed: false, ..Default::default() }
            }
            baseview::WindowEvent::Focused | baseview::WindowEvent::Unfocused => {
                let focused = matches!(event, baseview::WindowEvent::Focused);
                self.input.focused = focused;
                self.input.events.push(egui::Event::WindowFocused(focused));
                self.input.viewports.entry(self.viewport_id).or_default().focused = Some(focused);
                // Modifiers held when focus left are not held when it comes back, and nobody
                // will send the key-ups. Clearing here is what stops a phantom ⌘ from turning
                // every subsequent click into a command-click.
                if !focused {
                    self.input.modifiers = egui::Modifiers::default();
                }
                EventResponse { repaint: true, consumed: false, ..Default::default() }
            }
            // Nothing to tell egui: the window is going away and the state goes with it.
            baseview::WindowEvent::WillClose => {
                EventResponse { repaint: false, consumed: false, ..Default::default() }
            }
        }
    }

    /// Decide what [`Self::handle_platform_output`] should do, without doing it.
    ///
    /// Public because this is the testable half — and because a host that wants to open URLs,
    /// or that has a working macOS cursor path, can drive the effects itself.
    pub fn plan_platform_output(&mut self, output: egui::PlatformOutput) -> PlatformActions {
        let mut actions = PlatformActions::default();

        for command in output.commands {
            match command {
                egui::OutputCommand::CopyText(text) => actions.copy_text = Some(text),
                // baseview's clipboard is text-only. Reporting nothing is better than
                // reporting an empty copy that looks like it worked.
                egui::OutputCommand::CopyImage(_) => {}
                egui::OutputCommand::OpenUrl(url) => actions.open_url = Some(url.url),
            }
        }

        let cursor = cursor_icon(output.cursor_icon);
        if self.current_cursor_icon != Some(cursor) {
            self.current_cursor_icon = Some(cursor);
            actions.cursor = Some(cursor);
        }

        actions
    }

    /// Mirror of `egui_winit::State::handle_platform_output`.
    ///
    /// ⚠️ **The cursor call is compiled out on macOS**, and that is not a nicety:
    /// `baseview::Window::set_mouse_cursor` is `todo!()` in the macOS backend at rev
    /// `237d323c`, so calling it panics — inside Ableton's process, at whatever moment the
    /// pointer first crosses a text field. [`PlatformActions::cursor`] still reports the
    /// intent, so a host with its own AppKit path can act on it.
    pub fn handle_platform_output(
        &mut self,
        window: &mut baseview::Window<'_>,
        output: egui::PlatformOutput,
    ) {
        let actions = self.plan_platform_output(output);

        if let Some(text) = &actions.copy_text {
            baseview::copy_to_clipboard(text);
        }

        #[cfg(not(target_os = "macos"))]
        if let Some(cursor) = actions.cursor {
            window.set_mouse_cursor(cursor);
        }
        #[cfg(target_os = "macos")]
        let _ = window;

        // `actions.open_url` is deliberately not acted on — see the module docs.
    }
}

/// The window's logical rectangle in egui points, or `None` when it has no area.
///
/// `None` rather than a zero rect: a minimized window reports 0×0 on Windows, and handing egui
/// an empty screen rect moves every window it has laid out to the origin — the positions are
/// then wrong when the window comes back. `egui-winit` guards the same way.
fn screen_rect(window: &baseview::WindowInfo, pixels_per_point: f32) -> Option<Rect> {
    let px = window.physical_size();
    let size = vec2(px.width as f32, px.height as f32) / pixels_per_point;
    (size.x > 0.0 && size.y > 0.0).then(|| Rect::from_min_size(Pos2::ZERO, size))
}

fn to_points(p: baseview::Point, scale: f32) -> Pos2 {
    pos2(p.x as f32 * scale, p.y as f32 * scale)
}

fn hovered_files(data: &baseview::DropData) -> Vec<egui::HoveredFile> {
    match data {
        baseview::DropData::None => Vec::new(),
        baseview::DropData::Files(paths) => paths
            .iter()
            .map(|p| egui::HoveredFile { path: Some(p.clone()), ..Default::default() })
            .collect(),
    }
}

fn dropped_files(data: &baseview::DropData) -> Vec<egui::DroppedFile> {
    match data {
        baseview::DropData::None => Vec::new(),
        baseview::DropData::Files(paths) => paths
            .iter()
            .map(|p| egui::DroppedFile { path: Some(p.clone()), ..Default::default() })
            .collect(),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── the manifest invariant ──────────────────────────────────────────────
    //
    // Relocated here by #593 Tier 2. It used to live in `tests/baseview_input.rs`, the shim
    // that existed only to compile this module; Tier 2 declares the module in `lib.rs` and
    // deletes that shim, and the test came within one commit of being deleted with it (the
    // default suite went 1084 → 1083, which is how it was caught). It belongs with the module
    // whose correctness depends on it, not with the scaffolding that used to host it.

    /// The one thing the module's own tests cannot assert: that the `baseview` this crate
    /// depends on directly is the **same rev** the editor's window comes from.
    ///
    /// `vendor/nih_plug_egui`'s manifest already carries this rule for itself — "must match the
    /// rev egui-baseview master uses, or the two pull in SEPARATE copies of baseview and its
    /// `WindowHandle` / `PhySize` / window options become different types across the boundary".
    /// The translation has exactly the same exposure through `baseview::Event`, and now
    /// `wgpu_editor.rs` opens a window with one and reads events with the other.
    ///
    /// Cargo silently accepts the disagreement: two revs of one git dependency are two crates,
    /// both build, and the only symptom is an editor whose window delivers events this module's
    /// types do not match. Nothing here can catch that at the type level, because
    /// `nih_plug_egui` does not re-export `baseview` — so the check is on the manifests, which
    /// is where the fact lives.
    #[test]
    fn this_crate_and_the_vendored_editor_pin_the_same_baseview_rev() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let ours = baseview_rev(&root.join("Cargo.toml"));
        let editor = baseview_rev(&root.join("vendor/nih_plug_egui/Cargo.toml"));
        assert_eq!(
            ours, editor,
            "native/Cargo.toml pins baseview {ours}, vendor/nih_plug_egui/Cargo.toml pins \
             {editor} — two revs are two crates, and the editor's events would never match \
             this module's types. Move them together, never one at a time."
        );
    }

    /// The `rev = "…"` from the first `baseview = { … }` line in a manifest.
    fn baseview_rev(manifest: &std::path::Path) -> String {
        let text = std::fs::read_to_string(manifest)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("baseview ") || l.starts_with("baseview="))
            .unwrap_or_else(|| panic!("no baseview dependency in {}", manifest.display()));
        let (_, after) = line
            .split_once("rev = \"")
            .unwrap_or_else(|| panic!("baseview is not pinned to a rev in {}", manifest.display()));
        after
            .split_once('"')
            .unwrap_or_else(|| panic!("unterminated rev in {}", manifest.display()))
            .0
            .to_string()
    }

    fn no_mods() -> KbModifiers {
        KbModifiers::empty()
    }

    fn window(w: f64, h: f64, scale: f64) -> baseview::WindowInfo {
        baseview::WindowInfo::from_logical_size(baseview::Size::new(w, h), scale)
    }

    fn key_event(code: Code, key: KbKey, state: KeyState, mods: KbModifiers) -> KeyboardEvent {
        KeyboardEvent {
            state,
            key,
            code,
            location: keyboard_types::Location::Standard,
            modifiers: mods,
            repeat: false,
            is_composing: false,
        }
    }

    fn state_for(window: &baseview::WindowInfo) -> State {
        State::new(egui::Context::default(), egui::ViewportId::ROOT, window, None, None, Some(8192))
    }

    // ── mouse buttons ───────────────────────────────────────────────────────

    #[test]
    fn every_named_mouse_button_maps() {
        use baseview::MouseButton as B;
        use egui::PointerButton as P;
        assert_eq!(pointer_button(B::Left), Some(P::Primary));
        assert_eq!(pointer_button(B::Right), Some(P::Secondary));
        assert_eq!(pointer_button(B::Middle), Some(P::Middle));
        // The reference drops these two; egui has had `Extra1`/`Extra2` since 0.24, and a
        // mouse with side buttons is the common case, not the exotic one.
        assert_eq!(pointer_button(B::Back), Some(P::Extra1));
        assert_eq!(pointer_button(B::Forward), Some(P::Extra2));
    }

    #[test]
    fn an_unnamed_mouse_button_is_dropped_rather_than_guessed() {
        for n in [0u8, 3, 7, 255] {
            assert_eq!(pointer_button(baseview::MouseButton::Other(n)), None);
        }
    }

    #[test]
    fn the_three_primary_buttons_are_distinct() {
        use baseview::MouseButton as B;
        let mapped: Vec<_> =
            [B::Left, B::Right, B::Middle, B::Back, B::Forward].map(pointer_button).to_vec();
        for (i, a) in mapped.iter().enumerate() {
            for b in mapped.iter().skip(i + 1) {
                assert_ne!(a, b, "two baseview buttons collapsed onto one egui button");
            }
        }
    }

    // ── modifiers ───────────────────────────────────────────────────────────

    /// The correction this module exists for. On the Mac the shortcut modifier is **Command**,
    /// and the reference wires it to Control on every platform — so ⌘C reaches egui as a plain
    /// `C` (nothing copies) and ⌃C reaches it as a copy (a Unix habit that must not).
    #[test]
    fn command_is_meta_on_mac_and_control_elsewhere() {
        let meta = modifiers_for(KbModifiers::META, true);
        assert!(meta.command, "Meta must be the command modifier on macOS");
        assert!(meta.mac_cmd, "Meta must set mac_cmd on macOS");
        assert!(!meta.ctrl, "Meta is not Control");

        let ctrl_on_mac = modifiers_for(KbModifiers::CONTROL, true);
        assert!(!ctrl_on_mac.command, "Control must NOT be the command modifier on macOS");
        assert!(ctrl_on_mac.ctrl);
        assert!(!ctrl_on_mac.mac_cmd);

        let ctrl_elsewhere = modifiers_for(KbModifiers::CONTROL, false);
        assert!(ctrl_elsewhere.command, "Control is the command modifier off the Mac");
        assert!(ctrl_elsewhere.ctrl);
        assert!(!ctrl_elsewhere.mac_cmd, "mac_cmd must never be set off the Mac");

        let meta_elsewhere = modifiers_for(KbModifiers::META, false);
        assert!(!meta_elsewhere.command, "the Windows key is not a shortcut modifier");
        assert!(!meta_elsewhere.mac_cmd);
    }

    #[test]
    fn alt_and_shift_are_platform_independent() {
        for mac in [true, false] {
            let m = modifiers_for(KbModifiers::ALT | KbModifiers::SHIFT, mac);
            assert!(m.alt && m.shift);
            assert!(!m.ctrl && !m.command && !m.mac_cmd);
        }
    }

    #[test]
    fn no_modifiers_means_no_modifiers() {
        for mac in [true, false] {
            assert_eq!(modifiers_for(no_mods(), mac), egui::Modifiers::default());
        }
    }

    /// Bits egui has no field for (Caps Lock, Num Lock, Fn, AltGraph…) must not leak into one
    /// that it does — an AltGraph read as Alt turns every European `@` into a shortcut.
    #[test]
    fn modifier_bits_egui_does_not_model_are_ignored() {
        let noise = KbModifiers::CAPS_LOCK
            | KbModifiers::NUM_LOCK
            | KbModifiers::SCROLL_LOCK
            | KbModifiers::FN
            | KbModifiers::FN_LOCK
            | KbModifiers::ALT_GRAPH
            | KbModifiers::SYMBOL
            | KbModifiers::SYMBOL_LOCK
            | KbModifiers::HYPER
            | KbModifiers::SUPER;
        for mac in [true, false] {
            assert_eq!(modifiers_for(noise, mac), egui::Modifiers::default());
        }
    }

    #[test]
    fn all_four_modifiers_together_survive() {
        let all =
            KbModifiers::ALT | KbModifiers::SHIFT | KbModifiers::CONTROL | KbModifiers::META;
        let mac = modifiers_for(all, true);
        assert!(mac.alt && mac.shift && mac.ctrl && mac.mac_cmd && mac.command);
        let pc = modifiers_for(all, false);
        assert!(pc.alt && pc.shift && pc.ctrl && pc.command);
        assert!(!pc.mac_cmd);
    }

    /// X11 reports the modifier mask as it was *before* the event, so a Shift press arrives
    /// with the Shift bit clear. Reading the mask alone reports every modifier one keystroke
    /// late; folding the event's own key in fixes it in both directions.
    #[test]
    fn a_modifier_press_sets_its_own_bit_even_when_the_mask_lags() {
        for mac in [true, false] {
            let down =
                key_event(Code::ShiftLeft, KbKey::Shift, KeyState::Down, KbModifiers::empty());
            assert!(modifiers_from_key_event(&down, mac).shift, "shift down must read as held");

            // …and the release arrives with the bit still set, which must not stick.
            let up = key_event(Code::ShiftRight, KbKey::Shift, KeyState::Up, KbModifiers::SHIFT);
            assert!(!modifiers_from_key_event(&up, mac).shift, "shift up must read as released");
        }
    }

    #[test]
    fn a_command_press_sets_command_on_the_platform_that_has_one() {
        let meta_down = key_event(Code::MetaLeft, KbKey::Meta, KeyState::Down, no_mods());
        let on_mac = modifiers_from_key_event(&meta_down, true);
        assert!(on_mac.command && on_mac.mac_cmd);
        let elsewhere = modifiers_from_key_event(&meta_down, false);
        assert!(!elsewhere.command && !elsewhere.mac_cmd, "the Windows key is not ⌘");

        let ctrl_down = key_event(Code::ControlLeft, KbKey::Control, KeyState::Down, no_mods());
        assert!(modifiers_from_key_event(&ctrl_down, false).command);
        assert!(!modifiers_from_key_event(&ctrl_down, true).command);
        assert!(modifiers_from_key_event(&ctrl_down, true).ctrl, "⌃ is still ⌃ on the Mac");
    }

    /// A remapped Caps Lock reports `Code::ControlLeft` with a `Character` key; an unmapped
    /// modifier on some platforms reports `Key::Control` with an unhelpful code. Both spellings
    /// have to work or the correction only fires on one platform.
    #[test]
    fn a_modifier_is_recognised_by_code_or_by_logical_key() {
        let by_code = key_event(Code::AltLeft, KbKey::Unidentified, KeyState::Down, no_mods());
        assert!(modifiers_from_key_event(&by_code, false).alt);

        let by_key = key_event(Code::Unidentified, KbKey::Alt, KeyState::Down, no_mods());
        assert!(modifiers_from_key_event(&by_key, false).alt);
    }

    /// A non-modifier keypress must leave the mask exactly as reported — the fold-in is a
    /// correction for the key's *own* bit, not a licence to invent state.
    #[test]
    fn an_ordinary_key_does_not_disturb_the_reported_modifiers() {
        let ev = key_event(
            Code::KeyC,
            KbKey::Character("c".into()),
            KeyState::Down,
            KbModifiers::META | KbModifiers::SHIFT,
        );
        let m = modifiers_from_key_event(&ev, true);
        assert!(m.command && m.mac_cmd && m.shift);
        assert!(!m.alt && !m.ctrl);
    }

    // ── logical keys ────────────────────────────────────────────────────────

    #[test]
    fn the_named_navigation_keys_map() {
        use egui::Key;
        let cases: &[(KbKey, Key)] = &[
            (KbKey::ArrowDown, Key::ArrowDown),
            (KbKey::ArrowLeft, Key::ArrowLeft),
            (KbKey::ArrowRight, Key::ArrowRight),
            (KbKey::ArrowUp, Key::ArrowUp),
            (KbKey::Escape, Key::Escape),
            (KbKey::Tab, Key::Tab),
            (KbKey::Backspace, Key::Backspace),
            (KbKey::Enter, Key::Enter),
            (KbKey::Insert, Key::Insert),
            (KbKey::Delete, Key::Delete),
            (KbKey::Home, Key::Home),
            (KbKey::End, Key::End),
            (KbKey::PageUp, Key::PageUp),
            (KbKey::PageDown, Key::PageDown),
            (KbKey::Copy, Key::Copy),
            (KbKey::Cut, Key::Cut),
            (KbKey::Paste, Key::Paste),
            (KbKey::BrowserBack, Key::BrowserBack),
        ];
        for (from, to) in cases {
            assert_eq!(logical_key(from), Some(*to), "{from:?}");
        }
    }

    #[test]
    fn every_function_key_keyboard_types_has_maps() {
        use egui::Key;
        let cases: &[(KbKey, Key)] = &[
            (KbKey::F1, Key::F1),
            (KbKey::F2, Key::F2),
            (KbKey::F3, Key::F3),
            (KbKey::F4, Key::F4),
            (KbKey::F5, Key::F5),
            (KbKey::F6, Key::F6),
            (KbKey::F7, Key::F7),
            (KbKey::F8, Key::F8),
            (KbKey::F9, Key::F9),
            (KbKey::F10, Key::F10),
            (KbKey::F11, Key::F11),
            (KbKey::F12, Key::F12),
            (KbKey::F13, Key::F13),
            (KbKey::F14, Key::F14),
            (KbKey::F15, Key::F15),
            (KbKey::F16, Key::F16),
            (KbKey::F17, Key::F17),
            (KbKey::F18, Key::F18),
            (KbKey::F19, Key::F19),
            (KbKey::F20, Key::F20),
            (KbKey::F21, Key::F21),
            (KbKey::F22, Key::F22),
            (KbKey::F23, Key::F23),
            (KbKey::F24, Key::F24),
        ];
        for (from, to) in cases {
            assert_eq!(logical_key(from), Some(*to), "{from:?}");
        }
    }

    #[test]
    fn every_lowercase_letter_and_digit_maps() {
        use egui::Key;
        let letters = [
            Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J,
            Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T,
            Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        ];
        for (i, c) in ('a'..='z').enumerate() {
            assert_eq!(logical_key(&KbKey::Character(c.into())), Some(letters[i]), "{c}");
        }
        let digits = [
            Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6,
            Key::Num7, Key::Num8, Key::Num9,
        ];
        for (i, c) in ('0'..='9').enumerate() {
            assert_eq!(logical_key(&KbKey::Character(c.into())), Some(digits[i]), "{c}");
        }
    }

    /// **The shifted-letter regression.** The reference's table is lowercase only, so
    /// `Shift`+`Z` produced no `Key` event at all and ⇧⌘Z (redo) was a dead keystroke. An
    /// uppercase character must map to the same egui key as its lowercase twin.
    #[test]
    fn an_uppercase_letter_maps_to_the_same_key_as_its_lowercase_twin() {
        for (lower, upper) in ('a'..='z').zip('A'..='Z') {
            let l = logical_key(&KbKey::Character(lower.into()));
            let u = logical_key(&KbKey::Character(upper.into()));
            assert!(l.is_some(), "{lower} should map");
            assert_eq!(l, u, "{upper} must map to the same key as {lower}");
        }
    }

    /// Punctuation is where a hand-written table quietly stops halfway. Every ASCII
    /// punctuation mark egui has a `Key` for must reach it.
    #[test]
    fn the_punctuation_egui_names_all_maps() {
        use egui::Key;
        let cases: &[(char, Key)] = &[
            (' ', Key::Space),
            (':', Key::Colon),
            (',', Key::Comma),
            ('\\', Key::Backslash),
            ('/', Key::Slash),
            ('|', Key::Pipe),
            ('?', Key::Questionmark),
            ('!', Key::Exclamationmark),
            ('[', Key::OpenBracket),
            (']', Key::CloseBracket),
            ('{', Key::OpenCurlyBracket),
            ('}', Key::CloseCurlyBracket),
            ('`', Key::Backtick),
            ('-', Key::Minus),
            ('.', Key::Period),
            ('+', Key::Plus),
            ('=', Key::Equals),
            (';', Key::Semicolon),
            ('\'', Key::Quote),
        ];
        for (c, key) in cases {
            assert_eq!(logical_key(&KbKey::Character((*c).into())), Some(*key), "{c:?}");
        }
    }

    /// Composed text — a dead key resolving, an IME commit — has no single key identity. It
    /// must produce no `Key` event (the `Text` event carries it) rather than the key of its
    /// first character, which would fire a shortcut mid-composition.
    #[test]
    fn composed_multi_character_text_is_not_a_key() {
        for s in ["ab", "->", "こんにちは", "🙂🙂"] {
            assert_eq!(logical_key(&KbKey::Character(s.into())), None, "{s:?}");
        }
    }

    #[test]
    fn an_empty_or_unknown_key_is_dropped() {
        assert_eq!(logical_key(&KbKey::Character(String::new())), None);
        assert_eq!(logical_key(&KbKey::Unidentified), None);
        assert_eq!(logical_key(&KbKey::CapsLock), None);
        assert_eq!(logical_key(&KbKey::AudioVolumeUp), None);
        // A character egui has no key for is dropped, not forced onto a near-miss.
        assert_eq!(logical_key(&KbKey::Character("§".into())), None);
    }

    /// A `Character` payload is *typed text*, so it must not be read as a key *name*. Without
    /// the single-char restriction, `egui::Key::from_name` would read a pasted "Copy" as
    /// `Key::Copy` and fire a clipboard command.
    #[test]
    fn a_character_payload_is_never_read_as_a_key_name() {
        for s in ["Copy", "Cut", "Paste", "Enter", "Escape", "Space", "ArrowUp"] {
            assert_eq!(logical_key(&KbKey::Character(s.into())), None, "{s:?}");
        }
    }

    // ── physical keys ───────────────────────────────────────────────────────

    #[test]
    fn every_letter_and_digit_code_maps() {
        use egui::Key;
        let letter_codes = [
            Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF, Code::KeyG,
            Code::KeyH, Code::KeyI, Code::KeyJ, Code::KeyK, Code::KeyL, Code::KeyM, Code::KeyN,
            Code::KeyO, Code::KeyP, Code::KeyQ, Code::KeyR, Code::KeyS, Code::KeyT, Code::KeyU,
            Code::KeyV, Code::KeyW, Code::KeyX, Code::KeyY, Code::KeyZ,
        ];
        let letters = [
            Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J,
            Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T,
            Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        ];
        for (code, key) in letter_codes.iter().zip(letters) {
            assert_eq!(physical_key(*code), Some(key), "{code:?}");
        }

        let digit_codes = [
            Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5,
            Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
        ];
        let digits = [
            Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6,
            Key::Num7, Key::Num8, Key::Num9,
        ];
        for (code, key) in digit_codes.iter().zip(digits) {
            assert_eq!(physical_key(*code), Some(key), "{code:?}");
        }
    }

    /// egui's `Num0`..`Num9` are documented as "from main row **or numpad**", so the numpad
    /// must land on the same keys — otherwise typing a number on the numpad does nothing in a
    /// field that reads key events.
    #[test]
    fn the_numpad_folds_onto_the_main_row() {
        let numpad = [
            Code::Numpad0, Code::Numpad1, Code::Numpad2, Code::Numpad3, Code::Numpad4,
            Code::Numpad5, Code::Numpad6, Code::Numpad7, Code::Numpad8, Code::Numpad9,
        ];
        let main = [
            Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5,
            Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
        ];
        for (n, m) in numpad.iter().zip(main) {
            assert_eq!(physical_key(*n), physical_key(m), "{n:?} vs {m:?}");
        }
        // And the numpad's operators land where their main-row equivalents do.
        assert_eq!(physical_key(Code::NumpadAdd), Some(egui::Key::Plus));
        assert_eq!(physical_key(Code::NumpadSubtract), Some(egui::Key::Minus));
        assert_eq!(physical_key(Code::NumpadDivide), Some(egui::Key::Slash));
        assert_eq!(physical_key(Code::NumpadDecimal), Some(egui::Key::Period));
        assert_eq!(physical_key(Code::NumpadEnter), Some(egui::Key::Enter));
        assert_eq!(physical_key(Code::NumpadEqual), Some(egui::Key::Equals));
    }

    #[test]
    fn the_punctuation_codes_map_to_their_us_layout_characters() {
        use egui::Key;
        let cases: &[(Code, Key)] = &[
            (Code::Backquote, Key::Backtick),
            (Code::Backslash, Key::Backslash),
            (Code::BracketLeft, Key::OpenBracket),
            (Code::BracketRight, Key::CloseBracket),
            (Code::Comma, Key::Comma),
            (Code::Equal, Key::Equals),
            (Code::Minus, Key::Minus),
            (Code::Period, Key::Period),
            (Code::Quote, Key::Quote),
            (Code::Semicolon, Key::Semicolon),
            (Code::Slash, Key::Slash),
        ];
        for (code, key) in cases {
            assert_eq!(physical_key(*code), Some(*key), "{code:?}");
        }
    }

    #[test]
    fn every_function_key_code_maps_and_they_are_all_distinct() {
        let codes = [
            Code::F1, Code::F2, Code::F3, Code::F4, Code::F5, Code::F6, Code::F7, Code::F8,
            Code::F9, Code::F10, Code::F11, Code::F12, Code::F13, Code::F14, Code::F15,
            Code::F16, Code::F17, Code::F18, Code::F19, Code::F20, Code::F21, Code::F22,
            Code::F23, Code::F24,
        ];
        let mapped: Vec<_> = codes.iter().map(|c| physical_key(*c).unwrap()).collect();
        for (i, a) in mapped.iter().enumerate() {
            for (j, b) in mapped.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "{:?} and {:?} collapsed", codes[i], codes[j]);
            }
        }
        assert_eq!(physical_key(Code::F1), Some(egui::Key::F1));
        assert_eq!(physical_key(Code::F24), Some(egui::Key::F24));
    }

    /// A code egui has no name for must return `None`. Forcing `NumpadStar` onto, say,
    /// `Key::Num8` would make ⌘* fire ⌘8.
    #[test]
    fn a_code_egui_cannot_name_is_dropped() {
        for code in [
            Code::NumpadStar,
            Code::NumpadMultiply,
            Code::IntlYen,
            Code::IntlRo,
            Code::CapsLock,
            Code::ShiftLeft,
            Code::MetaRight,
            Code::AudioVolumeUp,
            Code::Unidentified,
        ] {
            assert_eq!(physical_key(code), None, "{code:?}");
        }
    }

    /// The point of `physical_key`: on AZERTY, the key in QWERTY's `Z` position still reports
    /// `Code::KeyZ` while the *logical* key is `w`. egui matches ⌘Z against the physical key,
    /// so both halves have to be present and they must disagree.
    #[test]
    fn a_non_qwerty_layout_still_reaches_the_qwerty_shortcut() {
        let azerty_undo =
            key_event(Code::KeyZ, KbKey::Character("w".into()), KeyState::Down, no_mods());
        assert_eq!(logical_key(&azerty_undo.key), Some(egui::Key::W));
        assert_eq!(physical_key(azerty_undo.code), Some(egui::Key::Z));
    }

    // ── scroll ──────────────────────────────────────────────────────────────

    #[test]
    fn a_line_scroll_passes_through_unscaled() {
        let (unit, d) = scroll_delta(baseview::ScrollDelta::Lines { x: -2.0, y: 3.0 }, 1.0);
        assert_eq!(unit, egui::MouseWheelUnit::Line);
        assert_eq!(d, vec2(-2.0, 3.0));
        // Lines are a count, not a length: the scale factor must not touch them.
        let (_, d2) = scroll_delta(baseview::ScrollDelta::Lines { x: -2.0, y: 3.0 }, 2.0);
        assert_eq!(d2, vec2(-2.0, 3.0));
    }

    /// baseview's `Pixels` comes only from macOS's `scrollingDeltaX/Y`, which are AppKit
    /// **points** — already logical. The reference divides them by the scale factor (a
    /// leftover from winit's logical→physical round trip), which halves trackpad scrolling on
    /// every Retina display. At the ordinary policy the factor here is 1.0 and nothing moves.
    #[test]
    fn a_precise_scroll_is_not_divided_by_the_retina_scale() {
        let (unit, d) = scroll_delta(baseview::ScrollDelta::Pixels { x: 12.0, y: -30.0 }, 1.0);
        assert_eq!(unit, egui::MouseWheelUnit::Point);
        assert_eq!(d, vec2(12.0, -30.0), "a 2x display must not halve the scroll");
    }

    /// The sign convention, pinned. The reference negates `delta.x` on macOS citing a winit
    /// bug whose issue it also links as closed; winit 0.30 passes `scrollingDeltaX` through
    /// and `egui-winit` 0.33 does not flip. Keeping the flip would make this editor scroll
    /// sideways opposite to the visual window in the same application.
    #[test]
    fn horizontal_scroll_is_not_flipped() {
        let (_, lines) = scroll_delta(baseview::ScrollDelta::Lines { x: 1.0, y: 0.0 }, 1.0);
        assert_eq!(lines.x, 1.0);
        let (_, px) = scroll_delta(baseview::ScrollDelta::Pixels { x: 1.0, y: 0.0 }, 1.0);
        assert_eq!(px.x, 1.0);
    }

    // ── cursors ─────────────────────────────────────────────────────────────

    /// Exhaustive by construction (the match has no `_` arm), so this test is really about the
    /// *other* direction: that nothing silently collapses onto `Default`.
    #[test]
    fn every_egui_cursor_maps_and_only_default_maps_to_default() {
        for icon in egui::CursorIcon::ALL {
            let mapped = cursor_icon(icon);
            if icon != egui::CursorIcon::Default {
                assert_ne!(
                    mapped,
                    baseview::MouseCursor::Default,
                    "{icon:?} fell back to a plain arrow"
                );
            }
        }
        assert_eq!(cursor_icon(egui::CursorIcon::Default), baseview::MouseCursor::Default);
    }

    #[test]
    fn the_resize_cursors_are_all_distinct() {
        use egui::CursorIcon as E;
        let resize = [
            E::ResizeHorizontal, E::ResizeVertical, E::ResizeNeSw, E::ResizeNwSe, E::ResizeEast,
            E::ResizeWest, E::ResizeNorth, E::ResizeSouth, E::ResizeNorthEast,
            E::ResizeNorthWest, E::ResizeSouthEast, E::ResizeSouthWest, E::ResizeColumn,
            E::ResizeRow,
        ];
        let mapped: Vec<_> = resize.iter().map(|c| cursor_icon(*c)).collect();
        for (i, a) in mapped.iter().enumerate() {
            for (j, b) in mapped.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "{:?} and {:?} collapsed", resize[i], resize[j]);
            }
        }
    }

    /// `NoDrop` ("this drop target will not take it") and `NotAllowed` ("this control is
    /// disabled") are different cursors, and baseview has both. The reference collapses them.
    #[test]
    fn no_drop_and_not_allowed_stay_distinct() {
        assert_eq!(cursor_icon(egui::CursorIcon::NoDrop), baseview::MouseCursor::PtrNotAllowed);
        assert_eq!(cursor_icon(egui::CursorIcon::NotAllowed), baseview::MouseCursor::NotAllowed);
    }

    #[test]
    fn hiding_the_cursor_is_hiding_it_not_defaulting_it() {
        assert_eq!(cursor_icon(egui::CursorIcon::None), baseview::MouseCursor::Hidden);
    }

    // ── clipboard commands ──────────────────────────────────────────────────

    #[test]
    fn the_shortcut_modifier_drives_cut_copy_paste() {
        let cmd = egui::Modifiers { command: true, ..Default::default() };
        assert_eq!(ClipboardCommand::recognise(cmd, egui::Key::X, false), Some(ClipboardCommand::Cut));
        assert_eq!(ClipboardCommand::recognise(cmd, egui::Key::C, false), Some(ClipboardCommand::Copy));
        assert_eq!(ClipboardCommand::recognise(cmd, egui::Key::V, false), Some(ClipboardCommand::Paste));
    }

    /// Without the shortcut modifier these are just letters. A bare `c` in a text field must
    /// type a c, not copy.
    #[test]
    fn a_bare_letter_is_not_a_clipboard_command() {
        let none = egui::Modifiers::default();
        for key in [egui::Key::X, egui::Key::C, egui::Key::V] {
            assert_eq!(ClipboardCommand::recognise(none, key, false), None, "{key:?}");
        }
        // Nor is it one under a modifier that is not the shortcut modifier.
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        assert_eq!(ClipboardCommand::recognise(shift, egui::Key::C, false), None);
    }

    /// On the Mac ⌃C must not copy — `command` is Meta there, so a Control-only modifier set
    /// never satisfies the check. This is the modifier correction showing up end to end.
    #[test]
    fn control_c_does_not_copy_on_a_mac() {
        let mac_ctrl = modifiers_for(KbModifiers::CONTROL, true);
        assert_eq!(ClipboardCommand::recognise(mac_ctrl, egui::Key::C, false), None);
        let mac_cmd = modifiers_for(KbModifiers::META, true);
        assert_eq!(
            ClipboardCommand::recognise(mac_cmd, egui::Key::C, false),
            Some(ClipboardCommand::Copy)
        );
    }

    #[test]
    fn the_dedicated_clipboard_keys_need_no_modifier() {
        let none = egui::Modifiers::default();
        assert_eq!(ClipboardCommand::recognise(none, egui::Key::Cut, false), Some(ClipboardCommand::Cut));
        assert_eq!(ClipboardCommand::recognise(none, egui::Key::Copy, false), Some(ClipboardCommand::Copy));
        assert_eq!(ClipboardCommand::recognise(none, egui::Key::Paste, false), Some(ClipboardCommand::Paste));
    }

    /// Windows' second set of bindings, and the fact that they are Windows-only: Shift+Delete
    /// is a *delete* everywhere else, and turning it into a cut would silently reroute text
    /// through the clipboard.
    #[test]
    fn the_windows_legacy_bindings_are_windows_only() {
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(
            ClipboardCommand::recognise(shift, egui::Key::Delete, true),
            Some(ClipboardCommand::Cut)
        );
        assert_eq!(
            ClipboardCommand::recognise(ctrl, egui::Key::Insert, true),
            Some(ClipboardCommand::Copy)
        );
        assert_eq!(
            ClipboardCommand::recognise(shift, egui::Key::Insert, true),
            Some(ClipboardCommand::Paste)
        );
        assert_eq!(ClipboardCommand::recognise(shift, egui::Key::Delete, false), None);
        assert_eq!(ClipboardCommand::recognise(ctrl, egui::Key::Insert, false), None);
    }

    // ── typed text ──────────────────────────────────────────────────────────

    #[test]
    fn ordinary_text_is_typed() {
        for s in ["a", "Z", "9", " ", "é", "→", "🙂", "hello"] {
            assert!(is_typed_text(s), "{s:?} should be typed");
        }
    }

    /// A Return reported as `Character("\r")` must not insert a carriage return *and* fire
    /// `Key::Enter`; macOS's function keys live in the private-use area, so an unfiltered Home
    /// key types `\u{f729}` into whatever has focus.
    #[test]
    fn control_characters_and_private_use_glyphs_are_not_typed() {
        for s in ["\r", "\n", "\t", "\u{7f}", "\u{1b}", "\u{f729}", "\u{e000}", "\u{100001}"] {
            assert!(!is_typed_text(s), "{s:?} must not be typed");
        }
        assert!(!is_typed_text(""), "an empty payload is not text");
        // One bad char poisons the whole payload rather than being silently stripped.
        assert!(!is_typed_text("ok\u{7f}"));
    }

    // ── screen rect / scale ─────────────────────────────────────────────────

    #[test]
    fn the_screen_rect_is_the_window_in_points() {
        let w = window(800.0, 600.0, 2.0);
        assert_eq!(w.physical_size(), baseview::PhySize::new(1600, 1200));
        let r = screen_rect(&w, 2.0).unwrap();
        assert_eq!(r.min, Pos2::ZERO);
        assert_eq!(r.width(), 800.0);
        assert_eq!(r.height(), 600.0);
    }

    /// A minimized window reports 0×0 on Windows. Handing egui an empty screen rect moves
    /// every window it has laid out to the origin, and they are still there when the window
    /// comes back — so no rect at all is the right answer.
    #[test]
    fn a_zero_sized_window_produces_no_screen_rect() {
        assert_eq!(screen_rect(&window(0.0, 0.0, 1.0), 1.0), None);
        assert_eq!(screen_rect(&window(800.0, 0.0, 1.0), 1.0), None);
        assert_eq!(screen_rect(&window(0.0, 600.0, 1.0), 1.0), None);
    }

    #[test]
    fn none_means_take_the_scale_factor_from_the_window() {
        let w = window(800.0, 600.0, 2.0);
        let s = state_for(&w);
        assert_eq!(s.resolve_pixels_per_point(&w), 2.0);
        assert_eq!(s.baseview_to_points(&w), 1.0, "no correction when the two agree");
    }

    #[test]
    fn a_pinned_scale_overrides_the_window_and_corrects_coordinates() {
        let w = window(800.0, 600.0, 2.0);
        let s = State::new(
            egui::Context::default(),
            egui::ViewportId::ROOT,
            &w,
            Some(1.0),
            None,
            None,
        );
        assert_eq!(s.resolve_pixels_per_point(&w), 1.0);
        // baseview still reports positions in *its* logical units (physical ÷ 2), so a point
        // it calls (400, 300) is (800, 600) in egui points.
        assert_eq!(s.baseview_to_points(&w), 2.0);
        assert_eq!(to_points(baseview::Point::new(400.0, 300.0), 2.0), pos2(800.0, 600.0));
    }

    /// A garbage scale factor must not divide the screen rect to infinity.
    #[test]
    fn a_nonsense_scale_factor_falls_back_to_one() {
        for bad in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
            let w = window(800.0, 600.0, 1.0);
            let s = State::new(
                egui::Context::default(),
                egui::ViewportId::ROOT,
                &w,
                Some(bad),
                None,
                None,
            );
            assert_eq!(s.resolve_pixels_per_point(&w), 1.0, "{bad} should fall back");
        }
    }

    // ── the four calls, end to end ──────────────────────────────────────────

    #[test]
    fn new_seeds_the_first_frame_with_the_windows_geometry() {
        let w = window(1280.0, 860.0, 2.0);
        let mut s = state_for(&w);
        let raw = s.take_egui_input(&w);
        let rect = raw.screen_rect.expect("the first frame must know how big the window is");
        assert_eq!(rect.width(), 1280.0);
        assert_eq!(rect.height(), 860.0);
        assert_eq!(raw.max_texture_side, Some(8192));
        assert_eq!(raw.viewport_id, egui::ViewportId::ROOT);
        assert_eq!(
            raw.viewports.get(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point,
            Some(2.0)
        );
        // baseview has not said anything about focus yet, so we must not claim it.
        assert!(!raw.focused);
    }

    #[test]
    fn take_egui_input_drains_the_queue_and_advances_time() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::CursorMoved {
                position: baseview::Point::new(10.0, 20.0),
                modifiers: no_mods(),
            }),
        );
        let first = s.take_egui_input(&w);
        assert_eq!(first.events, vec![egui::Event::PointerMoved(pos2(10.0, 20.0))]);
        assert!(first.time.is_some());

        let second = s.take_egui_input(&w);
        assert!(second.events.is_empty(), "events must not be delivered twice");
        assert!(second.time >= first.time);
    }

    #[test]
    fn a_click_carries_the_last_known_pointer_position() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        // A button with no prior motion has no position, so it produces nothing at all
        // rather than a click at the origin.
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::ButtonPressed {
                button: baseview::MouseButton::Left,
                modifiers: no_mods(),
            }),
        );
        assert!(s.take_egui_input(&w).events.is_empty());

        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::CursorMoved {
                position: baseview::Point::new(42.0, 99.0),
                modifiers: no_mods(),
            }),
        );
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::ButtonPressed {
                button: baseview::MouseButton::Left,
                modifiers: KbModifiers::SHIFT,
            }),
        );
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::ButtonReleased {
                button: baseview::MouseButton::Left,
                modifiers: KbModifiers::SHIFT,
            }),
        );
        let raw = s.take_egui_input(&w);
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        assert_eq!(
            raw.events,
            vec![
                egui::Event::PointerMoved(pos2(42.0, 99.0)),
                egui::Event::PointerButton {
                    pos: pos2(42.0, 99.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: shift,
                },
                egui::Event::PointerButton {
                    pos: pos2(42.0, 99.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: shift,
                },
            ]
        );
    }

    #[test]
    fn the_cursor_leaving_tells_egui_the_pointer_is_gone() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::CursorMoved {
                position: baseview::Point::new(5.0, 5.0),
                modifiers: no_mods(),
            }),
        );
        s.on_window_event(&w, &baseview::Event::Mouse(baseview::MouseEvent::CursorLeft));
        let raw = s.take_egui_input(&w);
        assert_eq!(raw.events.last(), Some(&egui::Event::PointerGone));

        // …and a click after that has no position again, so it produces nothing.
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::ButtonPressed {
                button: baseview::MouseButton::Left,
                modifiers: no_mods(),
            }),
        );
        assert!(s.take_egui_input(&w).events.is_empty());
    }

    #[test]
    fn a_resize_reaches_egui_as_a_new_screen_rect() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let bigger = window(1024.0, 768.0, 1.0);
        let r = s.on_window_event(
            &w,
            &baseview::Event::Window(baseview::WindowEvent::Resized(bigger)),
        );
        assert!(r.repaint && !r.consumed, "a resize is not consumed — the host needs it too");
        // Read back through the *new* window info, the way the host would.
        let raw = s.take_egui_input(&bigger);
        assert_eq!(raw.screen_rect.unwrap().width(), 1024.0);
    }

    #[test]
    fn focus_is_reported_both_ways_and_clears_stale_modifiers() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);

        // Hold ⌘ (or ⌃), then lose focus without ever seeing the key-up.
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::CursorMoved {
                position: baseview::Point::new(1.0, 1.0),
                modifiers: KbModifiers::CONTROL | KbModifiers::META,
            }),
        );
        s.on_window_event(&w, &baseview::Event::Window(baseview::WindowEvent::Unfocused));
        let raw = s.take_egui_input(&w);
        assert!(!raw.focused);
        assert!(raw.events.contains(&egui::Event::WindowFocused(false)));
        assert_eq!(raw.modifiers, egui::Modifiers::default(), "a lost key-up must not stick");

        s.on_window_event(&w, &baseview::Event::Window(baseview::WindowEvent::Focused));
        let raw = s.take_egui_input(&w);
        assert!(raw.focused);
        assert!(raw.events.contains(&egui::Event::WindowFocused(true)));
    }

    #[test]
    fn typing_produces_a_key_event_and_the_text() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyA,
                KbKey::Character("a".into()),
                KeyState::Down,
                no_mods(),
            )),
        );
        let raw = s.take_egui_input(&w);
        assert_eq!(
            raw.events,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: Some(egui::Key::A),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::Text("a".into()),
            ]
        );
    }

    #[test]
    fn a_key_release_produces_no_text() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyA,
                KbKey::Character("a".into()),
                KeyState::Up,
                no_mods(),
            )),
        );
        let raw = s.take_egui_input(&w);
        assert_eq!(raw.events.len(), 1, "a key-up must not type anything");
        assert!(matches!(raw.events[0], egui::Event::Key { pressed: false, .. }));
    }

    /// ⌘S must save, not type an "s" into the field that has focus. This is the check the
    /// reference gets right on Linux and wrong on macOS, because its `command` is Control.
    #[test]
    fn a_shortcut_does_not_also_type_its_letter() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyS,
                KbKey::Character("s".into()),
                KeyState::Down,
                cmd_mod(),
            )),
        );
        let raw = s.take_egui_input(&w);
        assert!(
            !raw.events.iter().any(|e| matches!(e, egui::Event::Text(_))),
            "a command-shortcut must not type its letter: {:?}",
            raw.events
        );
        assert!(raw.events.iter().any(|e| matches!(
            e,
            egui::Event::Key { key: egui::Key::S, pressed: true, .. }
        )));
    }

    /// Option/Alt is *not* a shortcut modifier for text purposes — on macOS it is how you
    /// type ø, ©, and every other alternate glyph.
    #[test]
    fn alt_still_types() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyO,
                KbKey::Character("ø".into()),
                KeyState::Down,
                KbModifiers::ALT,
            )),
        );
        let raw = s.take_egui_input(&w);
        assert!(raw.events.contains(&egui::Event::Text("ø".into())));
    }

    /// The shortcut modifier this build's platform actually uses, so the end-to-end tests
    /// exercise the real `cfg!` path rather than a hypothetical one.
    fn cmd_mod() -> KbModifiers {
        if cfg!(target_os = "macos") {
            KbModifiers::META
        } else {
            KbModifiers::CONTROL
        }
    }

    /// A clipboard shortcut is a clipboard event and **nothing else**. The reference emits the
    /// `Key` event too, so a user shortcut bound to ⌘C fires alongside the copy — which is how
    /// you get a keystroke that both copies and, say, opens the colour panel.
    #[test]
    fn cut_and_copy_become_clipboard_events_and_not_key_events() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyC,
                KbKey::Character("c".into()),
                KeyState::Down,
                cmd_mod(),
            )),
        );
        let raw = s.take_egui_input(&w);
        assert_eq!(raw.events, vec![egui::Event::Copy], "⌘C is a copy, not a copy AND a C");

        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyX,
                KbKey::Character("x".into()),
                KeyState::Down,
                cmd_mod(),
            )),
        );
        assert_eq!(s.take_egui_input(&w).events, vec![egui::Event::Cut]);
    }

    /// The reason `physical_key` is also the *fallback*: on a Cyrillic layout ⌘C reports the
    /// logical key `с`, which egui has no `Key` for at all. Without falling back to the
    /// physical `KeyC`, copy and paste simply do not exist on those layouts (egui#3653).
    #[test]
    fn a_cyrillic_layout_can_still_copy() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        // Nothing maps the logical key — that is the whole problem.
        assert_eq!(logical_key(&KbKey::Character("с".into())), None);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyC,
                KbKey::Character("с".into()), // U+0441, not the Latin c
                KeyState::Down,
                cmd_mod(),
            )),
        );
        assert_eq!(s.take_egui_input(&w).events, vec![egui::Event::Copy]);
    }

    /// The same fallback outside the clipboard: a key whose logical value egui cannot name
    /// still reaches egui as its physical key, which is what makes every other shortcut work
    /// on a non-Latin layout too.
    #[test]
    fn an_unnameable_logical_key_falls_back_to_its_physical_one() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::KeyS,
                KbKey::Character("ы".into()),
                KeyState::Down,
                cmd_mod(),
            )),
        );
        let raw = s.take_egui_input(&w);
        assert!(
            raw.events.iter().any(|e| matches!(
                e,
                egui::Event::Key { key: egui::Key::S, physical_key: Some(egui::Key::S), .. }
            )),
            "expected a fallback S, got {:?}",
            raw.events
        );
    }

    struct FixedClipboard(&'static str);
    impl ClipboardText for FixedClipboard {
        fn get(&mut self) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    fn paste_event() -> baseview::Event {
        baseview::Event::Keyboard(key_event(
            Code::KeyV,
            KbKey::Character("v".into()),
            KeyState::Down,
            cmd_mod(),
        ))
    }

    #[test]
    fn paste_is_inert_without_a_provider_and_works_with_one() {
        let w = window(800.0, 600.0, 1.0);

        let mut s = state_for(&w);
        s.on_window_event(&w, &paste_event());
        let raw = s.take_egui_input(&w);
        assert!(
            !raw.events.iter().any(|e| matches!(e, egui::Event::Paste(_))),
            "no provider must mean no paste, not an empty one"
        );

        let mut s = state_for(&w);
        s.set_clipboard(Box::new(FixedClipboard("pasted")));
        s.on_window_event(&w, &paste_event());
        assert_eq!(s.take_egui_input(&w).events, vec![egui::Event::Paste("pasted".into())]);
    }

    /// egui's text model is LF-only, so a paste out of a Windows editor would otherwise carry
    /// a stray `\r` on every line.
    #[test]
    fn a_paste_is_normalised_to_lf() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.set_clipboard(Box::new(FixedClipboard("one\r\ntwo\r\n")));
        s.on_window_event(&w, &paste_event());
        assert_eq!(s.take_egui_input(&w).events, vec![egui::Event::Paste("one\ntwo\n".into())]);
    }

    /// An empty clipboard produces no event at all: an empty paste still pushes undo history
    /// and marks the field dirty, which is a change the user did not make.
    #[test]
    fn an_empty_clipboard_pastes_nothing() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.set_clipboard(Box::new(FixedClipboard("")));
        s.on_window_event(&w, &paste_event());
        assert!(s.take_egui_input(&w).events.is_empty());
    }

    /// Tab always consumes, because egui uses it to move focus between widgets — handing it
    /// back to the host would move focus *and* do whatever the host does with Tab.
    #[test]
    fn tab_is_consumed_even_when_egui_wants_no_keys() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        assert!(!s.egui_ctx().wants_keyboard_input());
        let r = s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::Tab,
                KbKey::Tab,
                KeyState::Down,
                no_mods(),
            )),
        );
        assert!(r.consumed);

        // An ordinary key with no focused text field goes back to the host, which is what
        // lets a DAW keep its own shortcuts working while the editor is open.
        let r = s.on_window_event(
            &w,
            &baseview::Event::Keyboard(key_event(
                Code::Space,
                KbKey::Character(" ".into()),
                KeyState::Down,
                no_mods(),
            )),
        );
        assert!(!r.consumed, "the transport space bar must reach the host");
    }

    /// **All three** of baseview's `EventStatus` variants are reachable. The third one is the
    /// point: an earlier cut of this module could only ever produce `Captured`/`Ignored`, and
    /// since macOS and Windows both map *everything except* `AcceptDrop` onto
    /// "refuse the drop", drag-and-drop could hover but never land. Nothing here caught it,
    /// because X11 emits no drag events at all — so the tests fed `DragDropped` straight in,
    /// downstream of the gate that decides whether it is ever produced.
    #[test]
    fn the_event_response_maps_onto_all_three_baseview_statuses() {
        assert_eq!(
            EventResponse { consumed: true, repaint: true, ..Default::default() }.status(),
            baseview::EventStatus::Captured
        );
        assert_eq!(
            EventResponse { consumed: false, repaint: true, ..Default::default() }.status(),
            baseview::EventStatus::Ignored
        );
        assert_eq!(
            EventResponse {
                consumed: true,
                repaint: true,
                accept_drop: Some(baseview::DropEffect::Copy),
            }
            .status(),
            baseview::EventStatus::AcceptDrop(baseview::DropEffect::Copy)
        );
    }

    /// `accept_drop` outranks `consumed`: on a drag event the platform is asking "will you take
    /// this", and `Captured` is not a yes. Getting the precedence backwards would reintroduce
    /// the original bug for any drag we also mark consumed — which is every drag we accept.
    #[test]
    fn accepting_a_drop_outranks_being_consumed() {
        let r = EventResponse {
            consumed: true,
            repaint: true,
            accept_drop: Some(baseview::DropEffect::Copy),
        };
        assert_ne!(r.status(), baseview::EventStatus::Captured);
        assert_eq!(r.status(), baseview::EventStatus::AcceptDrop(baseview::DropEffect::Copy));
    }

    /// **The regression test for the finding.** Every drag event that carries files must answer
    /// `AcceptDrop`, or the platform kills the gesture: `DragEntered`/`DragMoved` gate whether
    /// `DragDropped` is ever emitted at all, and `DragDropped` itself gates whether
    /// `perform_drag_operation` reports success back to the source application.
    #[test]
    fn every_drag_event_carrying_files_accepts_the_drop() {
        let w = window(800.0, 600.0, 1.0);
        let data = || baseview::DropData::Files(vec![PathBuf::from("/models/qwen3-4b.gguf")]);
        let at = baseview::Point::new(100.0, 100.0);

        for (label, event) in [
            (
                "DragEntered",
                baseview::MouseEvent::DragEntered {
                    position: at,
                    modifiers: no_mods(),
                    data: data(),
                },
            ),
            (
                "DragMoved",
                baseview::MouseEvent::DragMoved {
                    position: at,
                    modifiers: no_mods(),
                    data: data(),
                },
            ),
            (
                "DragDropped",
                baseview::MouseEvent::DragDropped {
                    position: at,
                    modifiers: no_mods(),
                    data: data(),
                },
            ),
        ] {
            let mut s = state_for(&w);
            let r = s.on_window_event(&w, &baseview::Event::Mouse(event));
            assert_eq!(
                r.status(),
                baseview::EventStatus::AcceptDrop(baseview::DropEffect::Copy),
                "{label} refused the drop — the platform would cancel the gesture"
            );
        }
    }

    /// The converse, so "accept" does not degrade into "accept anything": a drag carrying data
    /// we cannot name is not claimed, and leaving is a real refusal.
    #[test]
    fn a_drag_we_cannot_use_is_not_accepted() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);

        let r = s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragEntered {
                position: baseview::Point::new(1.0, 1.0),
                modifiers: no_mods(),
                data: baseview::DropData::None,
            }),
        );
        assert_eq!(r.accept_drop, None);
        assert_eq!(r.status(), baseview::EventStatus::Ignored);

        let r = s.on_window_event(&w, &baseview::Event::Mouse(baseview::MouseEvent::DragLeft));
        assert_eq!(r.accept_drop, None);
    }

    /// No non-drag event may accept a drop. `..Default::default()` is what keeps that true as
    /// fields are added, and this is the test that notices if a hand-written literal drifts.
    #[test]
    fn no_ordinary_event_accepts_a_drop() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let events = [
            baseview::Event::Mouse(baseview::MouseEvent::CursorMoved {
                position: baseview::Point::new(5.0, 5.0),
                modifiers: no_mods(),
            }),
            baseview::Event::Mouse(baseview::MouseEvent::ButtonPressed {
                button: baseview::MouseButton::Left,
                modifiers: no_mods(),
            }),
            baseview::Event::Mouse(baseview::MouseEvent::WheelScrolled {
                delta: baseview::ScrollDelta::Lines { x: 0.0, y: 1.0 },
                modifiers: no_mods(),
            }),
            baseview::Event::Mouse(baseview::MouseEvent::CursorLeft),
            baseview::Event::Keyboard(key_event(
                Code::KeyA,
                KbKey::Character("a".into()),
                KeyState::Down,
                no_mods(),
            )),
            baseview::Event::Window(baseview::WindowEvent::Focused),
            baseview::Event::Window(baseview::WindowEvent::Unfocused),
            baseview::Event::Window(baseview::WindowEvent::WillClose),
        ];
        for e in events {
            let r = s.on_window_event(&w, &e);
            assert_eq!(r.accept_drop, None, "{e:?} must not accept a drop");
            assert_ne!(
                r.status(),
                baseview::EventStatus::AcceptDrop(baseview::DropEffect::Copy),
                "{e:?} must not accept a drop"
            );
        }
    }

    // ── drag and drop ───────────────────────────────────────────────────────

    #[test]
    fn a_dragged_file_becomes_a_hovered_file_and_then_a_dropped_one() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let path = PathBuf::from("/models/qwen3-4b.gguf");
        let data = baseview::DropData::Files(vec![path.clone()]);

        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragEntered {
                position: baseview::Point::new(100.0, 100.0),
                modifiers: no_mods(),
                data: data.clone(),
            }),
        );
        let raw = s.take_egui_input(&w);
        assert_eq!(raw.hovered_files.len(), 1);
        assert_eq!(raw.hovered_files[0].path.as_ref(), Some(&path));
        assert!(raw.dropped_files.is_empty());

        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragDropped {
                position: baseview::Point::new(100.0, 100.0),
                modifiers: no_mods(),
                data,
            }),
        );
        let raw = s.take_egui_input(&w);
        assert!(raw.hovered_files.is_empty(), "a dropped file is no longer hovering");
        assert_eq!(raw.dropped_files.len(), 1);
        assert_eq!(raw.dropped_files[0].path.as_ref(), Some(&path));
    }

    /// `hovered_files` is state, not a queue: egui expects the current hover set each frame,
    /// so a drag crossing the window must not accumulate one entry per motion event.
    #[test]
    fn dragging_across_the_window_does_not_accumulate_hovered_files() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let data = baseview::DropData::Files(vec![PathBuf::from("/a.gguf")]);
        for x in [10.0, 20.0, 30.0] {
            s.on_window_event(
                &w,
                &baseview::Event::Mouse(baseview::MouseEvent::DragMoved {
                    position: baseview::Point::new(x, 5.0),
                    modifiers: no_mods(),
                    data: data.clone(),
                }),
            );
        }
        assert_eq!(s.take_egui_input(&w).hovered_files.len(), 1);
    }

    /// **A stationary hover keeps hovering.** A real drag session stops sending events the
    /// moment the pointer stops moving — AppKit's `draggingUpdated:` fires on motion, not on a
    /// timer — so egui must see the same `hovered_files` on every frame in between, or the
    /// drop-target highlight flickers off while the user is still holding the file over the
    /// window.
    ///
    /// That works because `RawInput::take` **clones** `hovered_files` while `mem::take`-ing
    /// `events` and `dropped_files`. It is a deliberate asymmetry, and it is **egui's**, not
    /// ours: an egui bump could change it and nothing else here would notice, which is exactly
    /// the version-bump gap `tests/egui_popup_contract.rs` exists for. So it is pinned here.
    #[test]
    fn a_stationary_hover_survives_frames_with_no_events() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let path = PathBuf::from("/models/qwen3-4b.gguf");
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragEntered {
                position: baseview::Point::new(100.0, 100.0),
                modifiers: no_mods(),
                data: baseview::DropData::Files(vec![path.clone()]),
            }),
        );

        let first = s.take_egui_input(&w);
        assert_eq!(first.hovered_files.len(), 1);

        // The pointer has stopped. No further baseview events arrive — but egui keeps
        // repainting, and every one of those frames must still see the hover.
        for frame in 2..=4 {
            let raw = s.take_egui_input(&w);
            assert!(raw.events.is_empty(), "frame {frame} invented an event");
            assert_eq!(
                raw.hovered_files.len(),
                1,
                "the hover flickered off on frame {frame} with the file still held"
            );
            assert_eq!(raw.hovered_files[0].path.as_ref(), Some(&path));
        }

        // Only leaving ends it — which is the half already covered, asserted here against a
        // hover that has survived several frames rather than a fresh one.
        s.on_window_event(&w, &baseview::Event::Mouse(baseview::MouseEvent::DragLeft));
        assert!(s.take_egui_input(&w).hovered_files.is_empty());
    }

    /// The mirror property, and the reason the asymmetry above is not simply a bug in egui: a
    /// **drop** is delivered exactly once. `dropped_files` is `mem::take`n, so the next frame
    /// sees nothing — which is what stops a dropped `.gguf` from re-triggering a model load on
    /// every frame for as long as the window keeps repainting.
    #[test]
    fn a_dropped_file_is_delivered_exactly_once() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragDropped {
                position: baseview::Point::new(100.0, 100.0),
                modifiers: no_mods(),
                data: baseview::DropData::Files(vec![PathBuf::from("/models/qwen3-4b.gguf")]),
            }),
        );

        assert_eq!(s.take_egui_input(&w).dropped_files.len(), 1);
        for frame in 2..=4 {
            assert!(
                s.take_egui_input(&w).dropped_files.is_empty(),
                "frame {frame} re-delivered the drop — a model load would fire every frame"
            );
        }
    }

    #[test]
    fn a_drag_that_leaves_clears_the_hover() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragEntered {
                position: baseview::Point::new(1.0, 1.0),
                modifiers: no_mods(),
                data: baseview::DropData::Files(vec![PathBuf::from("/a.gguf")]),
            }),
        );
        s.on_window_event(&w, &baseview::Event::Mouse(baseview::MouseEvent::DragLeft));
        assert!(s.take_egui_input(&w).hovered_files.is_empty());
    }

    #[test]
    fn a_drag_carrying_nothing_is_not_claimed() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let r = s.on_window_event(
            &w,
            &baseview::Event::Mouse(baseview::MouseEvent::DragEntered {
                position: baseview::Point::new(1.0, 1.0),
                modifiers: no_mods(),
                data: baseview::DropData::None,
            }),
        );
        assert!(!r.consumed, "we cannot accept a drop we know nothing about");
        assert!(s.take_egui_input(&w).hovered_files.is_empty());
    }

    // ── platform output ─────────────────────────────────────────────────────

    fn output_with(cursor: egui::CursorIcon, commands: Vec<egui::OutputCommand>) -> egui::PlatformOutput {
        egui::PlatformOutput { cursor_icon: cursor, commands, ..Default::default() }
    }

    #[test]
    fn a_copy_command_is_carried_out_to_the_clipboard() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let a = s.plan_platform_output(output_with(
            egui::CursorIcon::Default,
            vec![egui::OutputCommand::CopyText("hello".into())],
        ));
        assert_eq!(a.copy_text.as_deref(), Some("hello"));
    }

    /// A URL is reported, never opened: `native/Cargo.toml` records that this interface opens
    /// no URLs, and a translation module is not the place to reverse that decision.
    #[test]
    fn a_url_is_reported_rather_than_opened() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let a = s.plan_platform_output(output_with(
            egui::CursorIcon::Default,
            vec![egui::OutputCommand::OpenUrl(egui::OpenUrl::same_tab("https://organon.art"))],
        ));
        assert_eq!(a.open_url.as_deref(), Some("https://organon.art"));
    }

    /// The cursor is pushed only when it changes. baseview's X11 backend dedupes too, but
    /// Windows' calls `SetCursor` unconditionally and macOS's is `todo!()` — so the change
    /// detection is ours to own.
    #[test]
    fn the_cursor_is_pushed_only_when_it_changes() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);

        // The very first frame always pushes: the window's cursor is whatever baseview left.
        let a = s.plan_platform_output(output_with(egui::CursorIcon::Default, vec![]));
        assert_eq!(a.cursor, Some(baseview::MouseCursor::Default));

        let a = s.plan_platform_output(output_with(egui::CursorIcon::Default, vec![]));
        assert_eq!(a.cursor, None, "an unchanged cursor must not be re-pushed");

        let a = s.plan_platform_output(output_with(egui::CursorIcon::Text, vec![]));
        assert_eq!(a.cursor, Some(baseview::MouseCursor::Text));

        // Two egui icons that map to the same baseview cursor are one change, not two.
        let a = s.plan_platform_output(output_with(egui::CursorIcon::PointingHand, vec![]));
        assert_eq!(a.cursor, Some(baseview::MouseCursor::Hand));
        let a = s.plan_platform_output(output_with(egui::CursorIcon::Grab, vec![]));
        assert_eq!(a.cursor, None, "PointingHand and Grab are the same baseview cursor");
    }

    #[test]
    fn an_image_copy_is_dropped_rather_than_faked() {
        let w = window(800.0, 600.0, 1.0);
        let mut s = state_for(&w);
        let img = egui::ColorImage::new([1, 1], vec![egui::Color32::RED]);
        let a = s.plan_platform_output(output_with(
            egui::CursorIcon::Default,
            vec![egui::OutputCommand::CopyImage(img)],
        ));
        assert_eq!(a.copy_text, None, "baseview's clipboard is text-only");
    }
}
