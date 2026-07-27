//! AutoIt key names to macOS virtual key codes.
//!
//! The values are Carbon's `kVK_*` constants, which are **layout-independent**
//! — `kVK_ANSI_A` is 0x00 whether the keyboard is QWERTY, AZERTY or Dvorak,
//! because it names a physical key position rather than a letter. That is why
//! this table only covers *named* keys, and ordinary characters go through
//! [`CGEventKeyboardSetUnicodeString`](super::input) instead: typing "ç" by
//! keycode would need a per-layout table and would still be wrong on the next
//! layout.
//!
//! # What is missing, and why
//!
//! AutoIt's vocabulary includes keys macOS does not have. `{PRINTSCREEN}` and
//! the `{BROWSER_*}` family are the notable ones. Rather than silently sending
//! nothing, [`lookup`] returns `None` and the caller turns that into an error
//! naming the key — see [`KeyMap`](crate::options::KeyMap) for the shortcut
//! translation that *is* available.

#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

/// A macOS virtual key code.
pub(crate) type KeyCode = u16;

// Modifier keys, needed by name for the down/up pairs.
pub(crate) const VK_COMMAND: KeyCode = 0x37;
pub(crate) const VK_SHIFT: KeyCode = 0x38;
pub(crate) const VK_OPTION: KeyCode = 0x3A;
pub(crate) const VK_CONTROL: KeyCode = 0x3B;
pub(crate) const VK_RIGHT_SHIFT: KeyCode = 0x3C;
pub(crate) const VK_RIGHT_OPTION: KeyCode = 0x3D;
pub(crate) const VK_RIGHT_CONTROL: KeyCode = 0x3E;

/// AutoIt name to virtual key code.
///
/// Names are matched case-insensitively, as AutoIt does, and are the same
/// vocabulary [`crate::keys`] validates against — so anything that parses is
/// looked up here, and anything absent is a genuine platform gap rather than a
/// typo.
const TABLE: &[(&str, KeyCode)] = &[
    // Editing and navigation
    ("ENTER", 0x24),
    ("TAB", 0x30),
    ("SPACE", 0x31),
    ("BACKSPACE", 0x33),
    ("BS", 0x33),
    ("ESCAPE", 0x35),
    ("ESC", 0x35),
    ("DELETE", 0x75),
    ("DEL", 0x75),
    ("HOME", 0x73),
    ("END", 0x77),
    ("PGUP", 0x74),
    ("PGDN", 0x79),
    ("LEFT", 0x7B),
    ("RIGHT", 0x7C),
    ("DOWN", 0x7D),
    ("UP", 0x7E),
    // `Insert` does not exist on Apple keyboards; the key in that position is
    // Help, which is what AutoIt's INSERT most nearly means here.
    ("INSERT", 0x72),
    ("INS", 0x72),
    // Function keys. Note the ordering is not sequential — these are physical
    // positions on the original Apple Extended Keyboard.
    ("F1", 0x7A),
    ("F2", 0x78),
    ("F3", 0x63),
    ("F4", 0x76),
    ("F5", 0x60),
    ("F6", 0x61),
    ("F7", 0x62),
    ("F8", 0x64),
    ("F9", 0x65),
    ("F10", 0x6D),
    ("F11", 0x67),
    ("F12", 0x6F),
    // Locks
    ("CAPSLOCK", 0x39),
    // Modifiers as one-shot names
    ("SHIFT", VK_SHIFT),
    ("LSHIFT", VK_SHIFT),
    ("RSHIFT", VK_RIGHT_SHIFT),
    ("CTRL", VK_CONTROL),
    ("LCTRL", VK_CONTROL),
    ("RCTRL", VK_RIGHT_CONTROL),
    ("ALT", VK_OPTION),
    ("LALT", VK_OPTION),
    ("RALT", VK_RIGHT_OPTION),
    ("LWIN", VK_COMMAND),
    ("RWIN", VK_COMMAND),
    // Numeric keypad
    ("NUMPAD0", 0x52),
    ("NUMPAD1", 0x53),
    ("NUMPAD2", 0x54),
    ("NUMPAD3", 0x55),
    ("NUMPAD4", 0x56),
    ("NUMPAD5", 0x57),
    ("NUMPAD6", 0x58),
    ("NUMPAD7", 0x59),
    ("NUMPAD8", 0x5B),
    ("NUMPAD9", 0x5C),
    ("NUMPADMULT", 0x43),
    ("NUMPADADD", 0x45),
    ("NUMPADSUB", 0x4E),
    ("NUMPADDIV", 0x4B),
    ("NUMPADDOT", 0x41),
    ("NUMPADENTER", 0x4C),
    // Volume, which macOS does expose as ordinary key codes.
    ("VOLUME_UP", 0x48),
    ("VOLUME_DOWN", 0x49),
    ("VOLUME_MUTE", 0x4A),
];

/// The ANSI key code for a character, for shortcuts only.
///
/// # Why shortcuts cannot go through the Unicode path
///
/// Ordinary characters are typed with `CGEventKeyboardSetUnicodeString`, which
/// is layout-independent and can produce anything — see the module docs. That
/// stops working the moment a command modifier is held: macOS resolves a key
/// *equivalent* from the event's virtual key code, and an event carrying key
/// code 0 with a Unicode string attached is not one it recognises.
///
/// Measured: `{CTRLDOWN}a{CTRLUP}` under
/// [`KeyMap::PortableShortcuts`](crate::options::KeyMap) posted a well-formed
/// Command-flagged event that TextEdit ignored entirely — nothing selected,
/// and no "a" typed either. Sending key code `kVK_ANSI_A` with the same flag
/// selects the document.
///
/// # The layout caveat
///
/// These are the ANSI *positions*. On a layout where the letters sit
/// elsewhere, `Cmd+A` here presses the key that is where A is on a US
/// keyboard. That matches how macOS itself binds key equivalents, and it is
/// the only option available: the alternative needs `UCKeyTranslate` against
/// the live layout, which would still be ambiguous for any character reachable
/// from more than one key.
#[must_use]
pub(crate) fn ansi(c: char) -> Option<KeyCode> {
    // Apple's `kVK_ANSI_*` constants. Not alphabetical, not sequential: these
    // are physical positions on the original Apple keyboard.
    Some(match c.to_ascii_lowercase() {
        'a' => 0x00,
        's' => 0x01,
        'd' => 0x02,
        'f' => 0x03,
        'h' => 0x04,
        'g' => 0x05,
        'z' => 0x06,
        'x' => 0x07,
        'c' => 0x08,
        'v' => 0x09,
        'b' => 0x0B,
        'q' => 0x0C,
        'w' => 0x0D,
        'e' => 0x0E,
        'r' => 0x0F,
        'y' => 0x10,
        't' => 0x11,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '6' => 0x16,
        '5' => 0x17,
        '=' => 0x18,
        '9' => 0x19,
        '7' => 0x1A,
        '-' => 0x1B,
        '8' => 0x1C,
        '0' => 0x1D,
        ']' => 0x1E,
        'o' => 0x1F,
        'u' => 0x20,
        '[' => 0x21,
        'i' => 0x22,
        'p' => 0x23,
        'l' => 0x25,
        'j' => 0x26,
        '\'' => 0x27,
        'k' => 0x28,
        ';' => 0x29,
        '\\' => 0x2A,
        ',' => 0x2B,
        '/' => 0x2C,
        'n' => 0x2D,
        'm' => 0x2E,
        '.' => 0x2F,
        '`' => 0x32,
        ' ' => 0x31,
        _ => return None,
    })
}

/// The virtual key code for an AutoIt key name, or `None` if macOS has no such
/// key.
#[must_use]
pub(crate) fn lookup(name: &str) -> Option<KeyCode> {
    TABLE
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, code)| *code)
}

/// Whether a name refers to a modifier that must be held rather than tapped.
#[must_use]
pub(crate) fn is_modifier(code: KeyCode) -> bool {
    matches!(
        code,
        VK_COMMAND
            | VK_SHIFT
            | VK_OPTION
            | VK_CONTROL
            | VK_RIGHT_SHIFT
            | VK_RIGHT_OPTION
            | VK_RIGHT_CONTROL
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_production_automation_actually_sends_are_all_present() {
        // Taken from the send strings in the .NET automation this crate is
        // being extracted from. If any of these is missing, a real flow breaks.
        for name in [
            "TAB",
            "ENTER",
            "ESC",
            "SPACE",
            "BACKSPACE",
            "HOME",
            "END",
            "UP",
            "DOWN",
            "PGUP",
            "F6",
            "SHIFT",
            "CTRL",
            "ALT",
            "LWIN",
        ] {
            assert!(lookup(name).is_some(), "{name} has no macOS key code");
        }
    }

    #[test]
    fn lookup_is_case_insensitive_like_autoit() {
        assert_eq!(lookup("tab"), lookup("TAB"));
        assert_eq!(lookup("Enter"), lookup("ENTER"));
    }

    #[test]
    fn function_keys_are_not_sequential() {
        // Easy to "tidy" into F1..F12 = 0x7A..0x85 and break everything: these
        // are physical positions on the Apple Extended Keyboard, not an
        // ordered range.
        assert_eq!(lookup("F1"), Some(0x7A));
        assert_eq!(lookup("F2"), Some(0x78));
        assert_eq!(lookup("F5"), Some(0x60));
        assert_ne!(lookup("F2"), lookup("F1").map(|c| c + 1));
    }

    #[test]
    fn aliases_agree_with_their_canonical_names() {
        assert_eq!(lookup("ESC"), lookup("ESCAPE"));
        assert_eq!(lookup("BS"), lookup("BACKSPACE"));
        assert_eq!(lookup("DEL"), lookup("DELETE"));
        assert_eq!(lookup("INS"), lookup("INSERT"));
    }

    #[test]
    fn keys_macos_does_not_have_report_absence_rather_than_a_wrong_code() {
        // Silently sending the wrong key is worse than failing: the automation
        // carries on believing it pressed something.
        for name in ["PRINTSCREEN", "BROWSER_BACK", "SCROLLLOCK", "APPSKEY"] {
            assert!(lookup(name).is_none(), "{name} should not resolve");
        }
    }

    #[test]
    fn every_modifier_name_maps_to_a_code_recognised_as_a_modifier() {
        for name in [
            "SHIFT", "LSHIFT", "RSHIFT", "CTRL", "LCTRL", "RCTRL", "ALT", "LALT", "RALT", "LWIN",
            "RWIN",
        ] {
            let code = lookup(name).unwrap_or_else(|| panic!("{name} missing"));
            assert!(
                is_modifier(code),
                "{name} -> {code:#x} not seen as modifier"
            );
        }
        // And an ordinary key is not one.
        assert!(!is_modifier(lookup("TAB").unwrap()));
    }

    #[test]
    fn the_shortcut_letters_automation_uses_have_ansi_codes() {
        // Copy, paste, cut, select-all, save, find, undo, redo, and the
        // browser/devtools shortcuts the real flows send.
        for c in "acvxszfyjnptw0123456789".chars() {
            assert!(ansi(c).is_some(), "{c} has no ANSI key code");
        }
        // Case does not matter: the shift flag carries that.
        assert_eq!(ansi('A'), ansi('a'));
        assert_eq!(ansi('A'), Some(0x00));
        assert_eq!(ansi('c'), Some(0x08));
        assert_eq!(ansi('v'), Some(0x09));
    }

    #[test]
    fn a_character_with_no_ansi_position_reports_absence() {
        // There is no key for these, so a shortcut naming one has to fail
        // rather than press something adjacent.
        for c in ['ç', 'ã', '€', '→'] {
            assert!(ansi(c).is_none(), "{c} should have no ANSI code");
        }
    }

    #[test]
    fn no_duplicate_names() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in TABLE {
            assert!(
                seen.insert(name.to_ascii_uppercase()),
                "{name} appears twice"
            );
        }
    }
}
