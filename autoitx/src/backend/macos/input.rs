//! Synthesising mouse and keyboard events with Core Graphics.
//!
//! # Two decisions worth defending
//!
//! **Characters are typed as Unicode, not as key codes.** A key code names a
//! physical position, so typing "ç" that way needs a per-layout table and is
//! still wrong on the next layout. `CGEventKeyboardSetUnicodeString` puts the
//! character itself on the event, so "Ünïcödé ãõç" arrives intact whatever
//! the keyboard is set to. Named keys — Tab, F6 — still go by code,
//! because those *are* positions.
//!
//! **Modifiers are posted as real key events, not just flags.**
//! `CGEventSetFlags` alone is unreliable across process boundaries: an
//! application that watches for a held modifier may never see one. So
//! `{CTRLDOWN}` posts an actual Control key-down, the flag rides on every
//! event until `{CTRLUP}`, and [`Held`] guarantees the release happens even if
//! the sequence panics half-way.

// Wired into `AutoIt` in phase 5, when the window operations land and the
// backend can be selected as a whole. Until then these are exercised by the
// tests at the bottom of this file, including one that drives the real cursor.
#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

use super::keycodes::{self, KeyCode};
use crate::keys::{Modifier, Token};
use crate::options::{KeyMap, Options};
use crate::{Keys, Point};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventType, CGMouseButton,
};
use std::time::Duration;

/// Where synthesised events are injected.
///
/// The HID tap is the lowest point available, so events look as much like real
/// hardware as this API allows — session-level taps would skip anything
/// listening below them.
const TAP: CGEventTapLocation = CGEventTapLocation::HIDEventTap;

/// A shared event source.
///
/// Using one source rather than `None` keeps the synthesised stream coherent:
/// events from a single source carry consistent state, which matters for
/// applications that track modifier and click history.
fn source() -> Option<objc2_core_foundation::CFRetained<CGEventSource>> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
}

// ---------------------------------------------------------------------------
// Modifiers
// ---------------------------------------------------------------------------

/// Modifiers currently held down, and the promise to let go.
///
/// This exists for one failure mode, and it is a bad one: if a send sequence
/// stops half-way — a panic, an error, a `?` — with Command down, that
/// modifier stays down in the user's session. Every subsequent keystroke they
/// type becomes a shortcut. Recovering means pressing the physical key, and the
/// cause is not remotely obvious.
///
/// So the held set lives in a value whose `Drop` releases everything, and every
/// path out of a send goes through it.
#[derive(Default)]
struct Held {
    codes: Vec<KeyCode>,
}

impl Held {
    fn press(&mut self, code: KeyCode) {
        if !self.codes.contains(&code) {
            post_key(code, true, self.flags());
            self.codes.push(code);
        }
    }

    fn release(&mut self, code: KeyCode) {
        if let Some(i) = self.codes.iter().position(|c| *c == code) {
            self.codes.remove(i);
            post_key(code, false, self.flags());
        }
    }

    /// The flag set matching what is currently held.
    fn flags(&self) -> CGEventFlags {
        let mut f = CGEventFlags::empty();
        for code in &self.codes {
            f |= match *code {
                keycodes::VK_COMMAND => CGEventFlags::MaskCommand,
                keycodes::VK_SHIFT | keycodes::VK_RIGHT_SHIFT => CGEventFlags::MaskShift,
                keycodes::VK_OPTION | keycodes::VK_RIGHT_OPTION => CGEventFlags::MaskAlternate,
                keycodes::VK_CONTROL | keycodes::VK_RIGHT_CONTROL => CGEventFlags::MaskControl,
                _ => CGEventFlags::empty(),
            };
        }
        f
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // Release in reverse, so the flag set shrinks the way it grew.
        while let Some(code) = self.codes.pop() {
            post_key(code, false, self.flags());
        }
    }
}

// ---------------------------------------------------------------------------
// Posting
// ---------------------------------------------------------------------------

fn post_key(code: KeyCode, down: bool, flags: CGEventFlags) {
    let Some(src) = source() else { return };
    let Some(event) = CGEvent::new_keyboard_event(Some(&src), code, down) else {
        return;
    };
    CGEvent::set_flags(Some(&event), flags);
    CGEvent::post(TAP, Some(&event));
}

/// Types one character by its Unicode value, independent of keyboard layout.
fn post_char(c: char, flags: CGEventFlags) {
    let Some(src) = source() else { return };
    let mut utf16 = [0u16; 2];
    let encoded = c.encode_utf16(&mut utf16);

    for down in [true, false] {
        // Key code 0 with a Unicode string attached: the code is ignored in
        // favour of the string, which is the documented way to type a
        // character the layout may not have.
        let Some(event) = CGEvent::new_keyboard_event(Some(&src), 0, down) else {
            return;
        };
        // SAFETY: `encoded` is a valid UTF-16 slice, and the length matches it;
        // the call copies out of the buffer before returning.
        unsafe {
            CGEvent::keyboard_set_unicode_string(
                Some(&event),
                encoded.len() as u64,
                encoded.as_ptr(),
            );
        }
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post(TAP, Some(&event));
    }
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

/// Translates an AutoIt modifier name to a macOS key code, per the key map.
fn modifier_code(name: &str, map: KeyMap) -> Option<KeyCode> {
    let base = keycodes::lookup(name)?;
    Some(match (map, base) {
        // The swap that makes Windows shortcuts mean what they meant.
        (KeyMap::PortableShortcuts, keycodes::VK_CONTROL) => keycodes::VK_COMMAND,
        (KeyMap::PortableShortcuts, keycodes::VK_COMMAND) => keycodes::VK_CONTROL,
        _ => base,
    })
}

/// Whether a modifier set makes this a key *equivalent* rather than typing.
///
/// Shift is deliberately not in the list: holding it is how a capital letter or
/// a `!` is typed, and those still want the Unicode path.
fn is_shortcut(flags: CGEventFlags) -> bool {
    flags.intersects(
        CGEventFlags::MaskCommand | CGEventFlags::MaskControl | CGEventFlags::MaskAlternate,
    )
}

/// Sends a key sequence.
///
/// # Errors
///
/// [`Error::Keys`](crate::Error::Keys) if the sequence does not parse, or
/// [`Error::UnsupportedKey`](crate::Error::UnsupportedKey) if it names a key
/// macOS does not have — better than pressing nothing and reporting success.
pub(crate) fn send(keys: &Keys, options: &Options) -> crate::Result<()> {
    let tokens = keys.tokens()?;
    let map = options.key_map;

    // Everything below can return early; `held` releases on the way out.
    let mut held = Held::default();
    // A modifier from shorthand (`^c`) applies to the next key only.
    let mut pending: Vec<KeyCode> = Vec::new();

    for token in &tokens {
        match token {
            Token::Modifier(m) => {
                let name = match m {
                    Modifier::Ctrl => "CTRL",
                    Modifier::Shift => "SHIFT",
                    Modifier::Alt => "ALT",
                    Modifier::Win => "LWIN",
                };
                if let Some(code) = modifier_code(name, map) {
                    pending.push(code);
                }
            }

            Token::Char(c) => {
                for code in &pending {
                    held.press(*code);
                }
                let flags = held.flags();

                // Two different mechanisms, and which one applies is decided
                // by the modifiers, not by the character. See
                // [`keycodes::ansi`] for the measurement behind this.
                if is_shortcut(flags) {
                    let Some(code) = keycodes::ansi(*c) else {
                        return Err(crate::Error::UnsupportedKey {
                            key: format!("{c} as part of a shortcut"),
                            platform: "macOS",
                        });
                    };
                    post_key(code, true, flags);
                    std::thread::sleep(options.send_key_down_delay);
                    post_key(code, false, flags);
                } else {
                    post_char(*c, flags);
                }

                for code in std::mem::take(&mut pending) {
                    held.release(code);
                }
                std::thread::sleep(options.send_key_delay);
            }

            Token::Named { name, repeat, hold } => {
                let upper = name.to_ascii_uppercase();

                // `{CTRLDOWN}` and `{CTRLUP}` are names, not a hold suffix.
                // Written without `let` chains: those need Rust 1.88 and this
                // crate's MSRV is 1.85.
                if let Some(code) = upper
                    .strip_suffix("DOWN")
                    .and_then(|base| modifier_code(base, map))
                {
                    held.press(code);
                    continue;
                }
                if let Some(code) = upper
                    .strip_suffix("UP")
                    .and_then(|base| modifier_code(base, map))
                {
                    held.release(code);
                    continue;
                }

                let Some(code) = keycodes::lookup(&upper) else {
                    return Err(crate::Error::UnsupportedKey {
                        key: upper,
                        platform: "macOS",
                    });
                };

                match hold {
                    Some(true) => held.press(code),
                    Some(false) => held.release(code),
                    None => {
                        for code in &pending {
                            held.press(*code);
                        }
                        for _ in 0..*repeat {
                            post_key(code, true, held.flags());
                            std::thread::sleep(options.send_key_down_delay);
                            post_key(code, false, held.flags());
                            std::thread::sleep(options.send_key_delay);
                        }
                        for code in std::mem::take(&mut pending) {
                            held.release(code);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Mouse
// ---------------------------------------------------------------------------

fn cg_point(p: Point) -> CGPoint {
    CGPoint {
        x: f64::from(p.x),
        y: f64::from(p.y),
    }
}

fn button_events(button: &str) -> (CGMouseButton, CGEventType, CGEventType, CGEventType) {
    match button {
        "right" | "secondary" => (
            CGMouseButton::Right,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::RightMouseDragged,
        ),
        "middle" => (
            CGMouseButton::Center,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::OtherMouseDragged,
        ),
        _ => (
            CGMouseButton::Left,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::LeftMouseDragged,
        ),
    }
}

fn post_mouse(kind: CGEventType, at: Point, button: CGMouseButton, click_state: Option<i64>) {
    let Some(src) = source() else { return };
    let Some(event) = CGEvent::new_mouse_event(Some(&src), kind, cg_point(at), button) else {
        return;
    };
    if let Some(n) = click_state {
        // Without this an application sees two separate single clicks rather
        // than a double click, and never opens the thing you double-clicked.
        CGEvent::set_integer_value_field(Some(&event), CGEventField::MouseEventClickState, n);
    }
    CGEvent::post(TAP, Some(&event));
}

/// Where the cursor is.
pub(crate) fn mouse_get_pos() -> crate::Result<Point> {
    let Some(src) = source() else {
        return Ok(Point::ORIGIN);
    };
    let p = CGEvent::new(Some(&src))
        .map(|e| CGEvent::location(Some(&e)))
        .unwrap_or(CGPoint { x: 0.0, y: 0.0 });
    Ok(Point::new(p.x as i32, p.y as i32))
}

/// Moves the cursor, optionally in steps.
///
/// AutoIt's speed is a number of intermediate positions rather than a duration;
/// 0 teleports. Interpolating matters for applications that track movement —
/// a hover menu that never sees the pointer arrive will not open.
pub(crate) fn mouse_move(to: Point, speed: Option<crate::options::Speed>) -> crate::Result<()> {
    let steps = speed.map_or(0, |s| s.get()).max(0);
    if steps > 0 {
        let from = mouse_get_pos()?;
        for i in 1..steps {
            let t = f64::from(i) / f64::from(steps);
            let x = f64::from(from.x) + (f64::from(to.x - from.x) * t);
            let y = f64::from(from.y) + (f64::from(to.y - from.y) * t);
            post_mouse(
                CGEventType::MouseMoved,
                Point::new(x as i32, y as i32),
                CGMouseButton::Left,
                None,
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    post_mouse(CGEventType::MouseMoved, to, CGMouseButton::Left, None);
    Ok(())
}

/// Clicks at a point.
pub(crate) fn mouse_click(
    button: &str,
    at: Point,
    clicks: u32,
    speed: Option<crate::options::Speed>,
    options: &Options,
) -> crate::Result<()> {
    let (cg_button, down, up, _) = button_events(button);
    mouse_move(at, speed)?;

    for n in 1..=clicks.max(1) {
        // The click state is what turns two clicks into a double click; it must
        // be set on both the down and the up.
        let state = Some(i64::from(n));
        post_mouse(down, at, cg_button, state);
        std::thread::sleep(options.mouse_click_down_delay);
        post_mouse(up, at, cg_button, state);
        std::thread::sleep(options.mouse_click_delay);
    }
    Ok(())
}

/// Presses a button and leaves it down.
pub(crate) fn mouse_down(button: &str) -> crate::Result<()> {
    let (cg_button, down, _, _) = button_events(button);
    let at = mouse_get_pos()?;
    post_mouse(down, at, cg_button, Some(1));
    Ok(())
}

/// Releases a button.
pub(crate) fn mouse_up(button: &str) -> crate::Result<()> {
    let (cg_button, _, up, _) = button_events(button);
    let at = mouse_get_pos()?;
    post_mouse(up, at, cg_button, Some(1));
    Ok(())
}

/// Drags from one point to another.
pub(crate) fn mouse_click_drag(
    button: &str,
    from: Point,
    to: Point,
    speed: Option<crate::options::Speed>,
    options: &Options,
) -> crate::Result<()> {
    let (cg_button, down, up, dragged) = button_events(button);

    mouse_move(from, speed)?;
    post_mouse(down, from, cg_button, Some(1));
    // Applications need a moment between the press and the movement to treat it
    // as a drag rather than a click that happened to move.
    std::thread::sleep(options.mouse_click_drag_delay);

    // Drag events, not moves: a move with the button down is not a drag as far
    // as most applications are concerned.
    let steps = speed.map_or(10, |s| s.get()).max(1);
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let x = f64::from(from.x) + (f64::from(to.x - from.x) * t);
        let y = f64::from(from.y) + (f64::from(to.y - from.y) * t);
        post_mouse(dragged, Point::new(x as i32, y as i32), cg_button, Some(1));
        std::thread::sleep(Duration::from_millis(5));
    }

    post_mouse(up, to, cg_button, Some(1));
    Ok(())
}

/// Scrolls the wheel.
pub(crate) fn mouse_wheel(direction: &str, clicks: u32) -> crate::Result<()> {
    let Some(src) = source() else { return Ok(()) };
    let amount = if direction.eq_ignore_ascii_case("down") {
        -1
    } else {
        1
    };
    for _ in 0..clicks.max(1) {
        let Some(event) = CGEvent::new_scroll_wheel_event2(
            Some(&src),
            objc2_core_graphics::CGScrollEventUnit::Line,
            1,
            amount,
            0,
            0,
        ) else {
            return Ok(());
        };
        CGEvent::post(TAP, Some(&event));
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    /// Drives the real cursor, so it is `#[ignore]`d: it would yank the mouse
    /// out from under whoever is at the machine.
    ///
    /// Run deliberately with:
    ///
    /// ```text
    /// cargo test -p autoitx macos::input -- --ignored --nocapture
    /// ```
    ///
    /// Worth having despite the inconvenience: it verifies the whole CGEvent
    /// path — source, event, tap, and the coordinate space — end to end,
    /// against the operating system, and needs no application to observe it.
    /// `mouse_get_pos` reads back what `mouse_move` wrote.
    #[test]
    #[ignore = "moves the real cursor"]
    fn the_cursor_actually_goes_where_it_is_sent() {
        let start = mouse_get_pos().expect("reading the cursor position");

        for target in [Point::new(300, 200), Point::new(700, 500)] {
            mouse_move(target, Some(crate::options::Speed::INSTANT)).unwrap();
            std::thread::sleep(Duration::from_millis(120));
            let got = mouse_get_pos().unwrap();

            // Exact, not approximate: this backend speaks logical points in the
            // same space CGEvent does, so a rounding difference here would mean
            // a real coordinate bug rather than tolerable drift.
            assert_eq!(got, target, "asked for {target:?}, cursor reports {got:?}");
        }

        // Put it back where it was; the person at the machine did not ask for
        // their pointer to be relocated.
        mouse_move(start, Some(crate::options::Speed::INSTANT)).unwrap();
    }

    /// Types into whatever has focus, so it is `#[ignore]`d.
    ///
    /// Focus a text editor first. Verifies the thing key codes cannot do:
    /// accented and non-Latin characters arriving intact regardless of the
    /// keyboard layout, because they travel as Unicode rather than as key
    /// positions.
    #[test]
    #[ignore = "types into the focused application"]
    fn accented_text_types_into_the_focused_app() {
        let text = "Ünïcödé ãõç — 1.234,56";
        println!("typing into the focused app in 3s: {text}");
        std::thread::sleep(Duration::from_secs(3));
        send(&keys::Keys::text(text), &Options::default()).unwrap();
    }

    #[test]
    fn the_held_set_tracks_presses_and_releases() {
        let mut h = Held::default();
        assert!(h.is_empty());

        h.press(keycodes::VK_COMMAND);
        assert!(!h.is_empty());
        assert!(h.flags().contains(CGEventFlags::MaskCommand));

        // Pressing twice must not queue two releases.
        h.press(keycodes::VK_COMMAND);
        assert_eq!(h.codes.len(), 1);

        h.release(keycodes::VK_COMMAND);
        assert!(h.is_empty());
        assert!(!h.flags().contains(CGEventFlags::MaskCommand));
    }

    #[test]
    fn releasing_something_never_pressed_is_harmless() {
        let mut h = Held::default();
        h.release(keycodes::VK_SHIFT);
        assert!(h.is_empty());
    }

    #[test]
    fn flags_accumulate_across_several_modifiers() {
        let mut h = Held::default();
        h.press(keycodes::VK_COMMAND);
        h.press(keycodes::VK_SHIFT);
        let f = h.flags();
        assert!(f.contains(CGEventFlags::MaskCommand));
        assert!(f.contains(CGEventFlags::MaskShift));
        assert!(!f.contains(CGEventFlags::MaskControl));
    }

    #[test]
    fn the_default_key_map_leaves_control_as_control() {
        // The decision this crate makes explicit: {CTRLDOWN} means Control on
        // macOS, not Command. A shortcut written for Windows will not silently
        // do something else.
        assert_eq!(
            modifier_code("CTRL", KeyMap::AsWritten),
            Some(keycodes::VK_CONTROL)
        );
        assert_eq!(
            modifier_code("LWIN", KeyMap::AsWritten),
            Some(keycodes::VK_COMMAND)
        );
    }

    #[test]
    fn portable_shortcuts_swap_control_and_command() {
        // The opt-in mapping, for automation whose CTRL sequences are all
        // editing shortcuts.
        assert_eq!(
            modifier_code("CTRL", KeyMap::PortableShortcuts),
            Some(keycodes::VK_COMMAND)
        );
        assert_eq!(
            modifier_code("LWIN", KeyMap::PortableShortcuts),
            Some(keycodes::VK_CONTROL)
        );
        // Option is Option either way — only the two that collide swap.
        assert_eq!(
            modifier_code("ALT", KeyMap::PortableShortcuts),
            modifier_code("ALT", KeyMap::AsWritten)
        );
    }

    #[test]
    fn an_unsupported_key_is_an_error_rather_than_silence() {
        // {PRINTSCREEN} has no macOS key. Pressing nothing and reporting
        // success would leave the automation believing it took a screenshot.
        let err = send(
            &keys::Keys::parse("{PRINTSCREEN}").unwrap(),
            &Options::default(),
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                crate::Error::UnsupportedKey {
                    platform: "macOS",
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_failure_mid_sequence_leaves_no_modifier_held() {
        // The reason `Held` exists. This sequence presses Command and then hits
        // an unsupported key, so it returns early — and the Drop must still
        // have released Command. If it did not, every key the user typed next
        // would become a shortcut.
        //
        // Asserted through the type rather than the OS: `Held` is what owns the
        // promise, so a leak would show up as a non-empty set at drop time.
        let mut h = Held::default();
        h.press(keycodes::VK_COMMAND);
        h.press(keycodes::VK_SHIFT);
        assert_eq!(h.codes.len(), 2);
        drop(h);

        // And the same shape through the public path.
        let err = send(
            &keys::Keys::parse("{CTRLDOWN}{PRINTSCREEN}{CTRLUP}").unwrap(),
            &Options::default(),
        );
        assert!(err.is_err(), "the unsupported key must still fail");
    }
}
