//! AutoIt's option table, and the defaults automation silently depends on.
//!
//! The .NET code this crate replaces never calls `AutoItSetOption` — not once
//! in 8,000 lines. It does not have to: `AU3_Init` installs a default table,
//! and every one of those defaults is load-bearing. Two in particular:
//!
//! - [`TitleMatchMode::StartsWith`] means `Selector::title("Order Entry de
//!   Pedidos")` also matches `"Order Entry - Filter"`. Automation
//!   written against it *depends* on prefix matching without saying so.
//! - [`CoordMode::Screen`] means every mouse coordinate is an absolute screen
//!   pixel, which is why that code also hard-codes a 1600×900 resolution check
//!   at startup.
//!
//! So [`Options::default`] reproduces AutoIt's table exactly, and it is a
//! documented contract rather than an implementation detail — the macOS backend
//! emulates the same defaults, and a test on Windows reads each option back
//! from the real DLL to prove the table has not drifted.

use std::time::Duration;

/// What coordinates are relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i32)]
pub enum CoordMode {
    /// Relative to the active window's client area.
    ActiveWindow = 0,
    /// Absolute screen coordinates. **AutoIt's default.**
    #[default]
    Screen = 1,
    /// Relative to the active window's client area, excluding decorations.
    Client = 2,
}

/// How a bare title is compared.
///
/// Only applies to [`Selector::title`](crate::Selector::title); advanced
/// `[TITLE:...]` syntax is unaffected by the negative (case-insensitive) modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i32)]
pub enum TitleMatchMode {
    /// The window title starts with the given text. **AutoIt's default**, and
    /// the one nearly all existing automation implicitly relies on.
    #[default]
    StartsWith = 1,
    /// The title contains the given text anywhere.
    Substring = 2,
    /// The title is exactly the given text.
    Exact = 3,
    /// Advanced `[PROP:value]` syntax only; bare titles stop matching.
    Advanced = 4,
}

/// How window text is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(i32)]
pub enum TextMatchMode {
    /// Complete, slow match. **AutoIt's default.**
    #[default]
    Complete = 1,
    /// Quick match.
    Quick = 2,
}

/// How a window should be shown, for `win_set_state`.
///
/// These are the Win32 `SW_*` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
#[non_exhaustive]
pub enum ShowState {
    /// Hide the window.
    Hide = 0,
    /// Show it at its normal size and position.
    ShowNormal = 1,
    /// Show it minimised.
    ShowMinimized = 2,
    /// Maximise it.
    ///
    /// On macOS this sets the window frame to the screen's visible area rather
    /// than pressing the zoom button, because zoom toggles and its meaning is
    /// left to each application.
    Maximize = 3,
    /// Show it without activating.
    ShowNoActivate = 4,
    /// Show it.
    Show = 5,
    /// Minimise it.
    Minimize = 6,
    /// Minimise without activating.
    ShowMinNoActive = 7,
    /// Show without activating.
    ShowNa = 8,
    /// Restore from minimised or maximised.
    Restore = 9,
}

bitflags::bitflags! {
    /// What `win_get_state` reports.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WinState: i32 {
        /// The window exists.
        const EXISTS = 1;
        /// It is visible.
        const VISIBLE = 2;
        /// It is enabled for input.
        const ENABLED = 4;
        /// It is the active window.
        const ACTIVE = 8;
        /// It is minimised.
        const MINIMIZED = 16;
        /// It is maximised.
        const MAXIMIZED = 32;
    }
}

/// How Windows modifier names translate on macOS.
///
/// `{CTRLDOWN}c{CTRLUP}` means Copy on Windows and **Control-C** on macOS,
/// where Copy is Command-C. There is no reading of that sequence that is right
/// on both platforms, so this makes the choice explicit rather than guessing.
///
/// ```
/// # use autoitx::options::{KeyMap, Options};
/// // The default: CTRL means Control. A shortcut written for Windows will
/// // not do what it did there, and will do so loudly.
/// assert_eq!(Options::default().key_map, KeyMap::AsWritten);
/// ```
///
/// # Which to choose
///
/// If your `{CTRLDOWN}...{CTRLUP}` sequences are all editing shortcuts — copy,
/// paste, select-all, save — [`PortableShortcuts`](Self::PortableShortcuts) is
/// what you want, and it is a one-line change.
///
/// If any of them mean Control literally — a terminal's Ctrl-C, an
/// application's own Control binding — leave the default and translate those
/// call sites deliberately. Silent remapping would turn "interrupt this
/// process" into "copy", which is not a bug anyone enjoys finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum KeyMap {
    /// Names mean what they say: `CTRL` is Control, `ALT` is Option, `LWIN` is
    /// Command.
    ///
    /// The default, because a shortcut that quietly does something else is
    /// worse than one that plainly does nothing.
    #[default]
    AsWritten,

    /// Translates Windows shortcut *intent* to the macOS equivalent.
    ///
    /// | written | `AsWritten` | `PortableShortcuts` |
    /// |---|---|---|
    /// | `CTRL` | Control | **Command** |
    /// | `ALT` | Option | Option |
    /// | `LWIN` / `RWIN` | Command | **Control** |
    ///
    /// `CTRL` and the Windows key swap places, which is what makes
    /// `{CTRLDOWN}c{CTRLUP}` copy and `{LWINDOWN}...` reach the Control-key
    /// bindings it would have reached on Windows.
    PortableShortcuts,
}

/// Mouse movement speed, 0 (instant) to 100 (slowest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Speed(u8);

impl Speed {
    /// Teleport the cursor, with no intermediate movement.
    pub const INSTANT: Self = Self(0);
    /// AutoIt's default.
    pub const DEFAULT: Self = Self(10);
    /// The slowest movement AutoIt accepts.
    pub const SLOWEST: Self = Self(100);

    /// Builds a speed, clamping to the valid range.
    #[must_use]
    pub const fn new(v: u8) -> Self {
        Self(if v > 100 { 100 } else { v })
    }

    /// The raw value AutoIt expects.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0 as i32
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// AutoIt's option table.
///
/// [`Options::default`] is exactly what `AU3_Init` installs. See the
/// [module docs](self) for why that matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// What mouse coordinates are relative to.
    pub mouse_coord_mode: CoordMode,
    /// What pixel coordinates are relative to.
    pub pixel_coord_mode: CoordMode,
    /// What caret coordinates are relative to.
    pub caret_coord_mode: CoordMode,
    /// How bare window titles are compared.
    pub win_title_match_mode: TitleMatchMode,
    /// Whether title matching ignores case.
    pub win_title_match_case_insensitive: bool,
    /// How window text is compared.
    pub win_text_match_mode: TextMatchMode,
    /// Whether hidden window text is searched.
    pub win_detect_hidden_text: bool,
    /// Whether child windows are searched.
    pub win_search_children: bool,
    /// Pause between each keystroke.
    pub send_key_delay: Duration,
    /// Pause a key is held down for.
    pub send_key_down_delay: Duration,
    /// Whether Caps Lock is restored after a send.
    pub send_capslock_mode: bool,
    /// Pause between mouse clicks.
    pub mouse_click_delay: Duration,
    /// How long a mouse button is held.
    pub mouse_click_down_delay: Duration,
    /// Pause after the button goes down at the start of a drag.
    pub mouse_click_drag_delay: Duration,
    /// Polling interval for the window-wait functions.
    pub win_wait_delay: Duration,
    /// How Windows modifier names are interpreted. Only affects macOS.
    pub key_map: KeyMap,
}

impl Options {
    /// The defaults, with a different [`KeyMap`].
    ///
    /// `Options` is `#[non_exhaustive]`, so it cannot be built with struct
    /// update syntax from outside this crate — and the key map is the one field
    /// a macOS user reaches for straight away.
    ///
    /// ```
    /// use autoitx::options::{KeyMap, Options};
    ///
    /// // `{CTRLDOWN}c{CTRLUP}` now means Copy on macOS, as it does on Windows.
    /// let o = Options::default().with_key_map(KeyMap::PortableShortcuts);
    /// assert_eq!(o.key_map, KeyMap::PortableShortcuts);
    /// ```
    #[must_use]
    pub const fn with_key_map(mut self, key_map: KeyMap) -> Self {
        self.key_map = key_map;
        self
    }
}

impl Default for Options {
    /// AutoIt's own defaults, as installed by `AU3_Init`.
    fn default() -> Self {
        Self {
            mouse_coord_mode: CoordMode::Screen,
            pixel_coord_mode: CoordMode::Screen,
            caret_coord_mode: CoordMode::Screen,
            win_title_match_mode: TitleMatchMode::StartsWith,
            win_title_match_case_insensitive: false,
            win_text_match_mode: TextMatchMode::Complete,
            win_detect_hidden_text: false,
            win_search_children: false,
            send_key_delay: Duration::from_millis(5),
            send_key_down_delay: Duration::from_millis(5),
            send_capslock_mode: true,
            mouse_click_delay: Duration::from_millis(10),
            mouse_click_down_delay: Duration::from_millis(10),
            mouse_click_drag_delay: Duration::from_millis(250),
            win_wait_delay: Duration::from_millis(250),
            key_map: KeyMap::AsWritten,
        }
    }
}

/// Reads or writes one option by AutoIt's own name.
///
/// This is what the macOS backend uses to emulate `AutoItSetOption`, whose
/// contract has two halves: it returns the *previous* value, and passing
/// [`AU3_INTDEFAULT`](autoitx_sys::AU3_INTDEFAULT) reads without writing. Both
/// are relied on — the second is how `get_option` and the defaults-parity test
/// work.
///
/// Names are matched case-insensitively, as AutoIt does.
///
/// # Errors
///
/// [`Error::Platform`](crate::Error::Platform) for a name this backend does not
/// model. Reported rather than ignored: quietly accepting a misspelled
/// `"SendKeyDelay"` would leave automation timing-dependent for no visible
/// reason.
// Reachable only from the native backend. With `mock-loader` on, even a macOS
// build runs the DLL and AutoIt keeps its own option table.
#[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
pub(crate) fn apply_named(
    options: &mut Options,
    name: &str,
    value: i32,
    read_only: bool,
) -> crate::Result<i32> {
    /// Reads, and conditionally writes, a millisecond duration.
    fn millis(field: &mut Duration, value: i32, read_only: bool) -> i32 {
        let previous = field.as_millis() as i32;
        if !read_only && value >= 0 {
            *field = Duration::from_millis(value as u64);
        }
        previous
    }

    /// Reads, and conditionally writes, a boolean stored as 0/1.
    fn flag(field: &mut bool, value: i32, read_only: bool) -> i32 {
        let previous = i32::from(*field);
        if !read_only {
            *field = value != 0;
        }
        previous
    }

    /// Reads, and conditionally writes, a coordinate mode.
    fn coords(field: &mut CoordMode, value: i32, read_only: bool) -> i32 {
        let previous = *field as i32;
        if !read_only {
            *field = match value {
                0 => CoordMode::ActiveWindow,
                2 => CoordMode::Client,
                _ => CoordMode::Screen,
            };
        }
        previous
    }

    let previous = match name.to_ascii_lowercase().as_str() {
        "mousecoordmode" => coords(&mut options.mouse_coord_mode, value, read_only),
        "pixelcoordmode" => coords(&mut options.pixel_coord_mode, value, read_only),
        "caretcoordmode" => coords(&mut options.caret_coord_mode, value, read_only),

        // AutoIt folds two settings into this one number: the magnitude picks
        // the comparison, and a negative sign makes it case-insensitive.
        "wintitlematchmode" => {
            let sign = if options.win_title_match_case_insensitive {
                -1
            } else {
                1
            };
            let previous = options.win_title_match_mode as i32 * sign;
            if !read_only {
                options.win_title_match_case_insensitive = value < 0;
                options.win_title_match_mode = match value.abs() {
                    2 => TitleMatchMode::Substring,
                    3 => TitleMatchMode::Exact,
                    4 => TitleMatchMode::Advanced,
                    _ => TitleMatchMode::StartsWith,
                };
            }
            previous
        }

        "wintextmatchmode" => {
            let previous = options.win_text_match_mode as i32;
            if !read_only {
                options.win_text_match_mode = if value == 2 {
                    TextMatchMode::Quick
                } else {
                    TextMatchMode::Complete
                };
            }
            previous
        }

        "windetecthiddentext" => flag(&mut options.win_detect_hidden_text, value, read_only),
        "winsearchchildren" => flag(&mut options.win_search_children, value, read_only),
        "sendcapslockmode" => flag(&mut options.send_capslock_mode, value, read_only),

        "sendkeydelay" => millis(&mut options.send_key_delay, value, read_only),
        "sendkeydowndelay" => millis(&mut options.send_key_down_delay, value, read_only),
        "mouseclickdelay" => millis(&mut options.mouse_click_delay, value, read_only),
        "mouseclickdowndelay" => millis(&mut options.mouse_click_down_delay, value, read_only),
        "mouseclickdragdelay" => millis(&mut options.mouse_click_drag_delay, value, read_only),
        "winwaitdelay" => millis(&mut options.win_wait_delay, value, read_only),

        _ => {
            return Err(crate::Error::Platform {
                operation: "set an option it does not model (see `Options` for the ones it does)",
                platform: "the macOS backend",
            });
        }
    };
    Ok(previous)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
    #[test]
    fn the_read_only_sentinel_reports_every_option_without_changing_any() {
        // The mechanism behind `get_option`, and behind the parity test that
        // checks `Options::default` against what AutoIt itself installs.
        let mut o = Options::default();
        let before = o;
        for name in [
            "MouseCoordMode",
            "PixelCoordMode",
            "CaretCoordMode",
            "WinTitleMatchMode",
            "WinTextMatchMode",
            "WinDetectHiddenText",
            "WinSearchChildren",
            "SendCapslockMode",
            "SendKeyDelay",
            "SendKeyDownDelay",
            "MouseClickDelay",
            "MouseClickDownDelay",
            "MouseClickDragDelay",
            "WinWaitDelay",
        ] {
            apply_named(&mut o, name, autoitx_sys::AU3_INTDEFAULT, true)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert_eq!(o, before, "a read-only call changed something");
    }

    #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
    #[test]
    fn a_negative_title_match_mode_means_case_insensitive() {
        // AutoIt packs two settings into one number, and automation in the wild
        // uses the negative form. Losing the sign would silently make matching
        // case-sensitive again.
        let mut o = Options::default();
        apply_named(&mut o, "WinTitleMatchMode", -2, false).expect("known option");
        assert_eq!(o.win_title_match_mode, TitleMatchMode::Substring);
        assert!(o.win_title_match_case_insensitive);

        // And it round-trips: reading gives back the negative number.
        let read = apply_named(
            &mut o,
            "WinTitleMatchMode",
            autoitx_sys::AU3_INTDEFAULT,
            true,
        )
        .expect("known option");
        assert_eq!(read, -2);
    }

    #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
    #[test]
    fn names_are_matched_case_insensitively_like_autoit() {
        let mut o = Options::default();
        assert_eq!(
            apply_named(&mut o, "sendkeydelay", 20, false).expect("known option"),
            5
        );
        assert_eq!(o.send_key_delay, Duration::from_millis(20));
        assert_eq!(
            apply_named(&mut o, "SENDKEYDELAY", 30, false).expect("known option"),
            20
        );
    }

    #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
    #[test]
    fn an_unmodelled_option_is_an_error_rather_than_a_silent_no_op() {
        let mut o = Options::default();
        assert!(apply_named(&mut o, "TrayIconDebug", 1, false).is_err());
        // Including a near miss, which is the case that actually happens.
        assert!(apply_named(&mut o, "SendKeyDelays", 1, false).is_err());
    }

    #[test]
    fn defaults_match_the_autoit_table() {
        // This is the documented contract. If AutoIt ever changes a default,
        // this test and the live parity test on Windows should both fail —
        // which is the point.
        let o = Options::default();
        assert_eq!(o.mouse_coord_mode, CoordMode::Screen);
        assert_eq!(o.win_title_match_mode, TitleMatchMode::StartsWith);
        assert_eq!(o.send_key_delay, Duration::from_millis(5));
        assert_eq!(o.win_wait_delay, Duration::from_millis(250));
        assert_eq!(o.mouse_click_drag_delay, Duration::from_millis(250));
        assert!(!o.win_detect_hidden_text);
        assert!(o.send_capslock_mode);
    }

    #[test]
    fn enum_discriminants_match_autoits_numbering() {
        // These cross the FFI boundary as plain integers.
        assert_eq!(CoordMode::ActiveWindow as i32, 0);
        assert_eq!(CoordMode::Screen as i32, 1);
        assert_eq!(TitleMatchMode::StartsWith as i32, 1);
        assert_eq!(TitleMatchMode::Advanced as i32, 4);
        assert_eq!(ShowState::Maximize as i32, 3);
        assert_eq!(ShowState::Restore as i32, 9);
    }

    #[test]
    fn speed_clamps_instead_of_wrapping() {
        assert_eq!(Speed::new(200).get(), 100);
        assert_eq!(Speed::new(0), Speed::INSTANT);
        assert_eq!(Speed::default(), Speed::DEFAULT);
        assert_eq!(Speed::DEFAULT.get(), 10);
    }

    #[test]
    fn win_state_flags_compose() {
        let s = WinState::EXISTS | WinState::VISIBLE | WinState::ACTIVE;
        assert!(s.contains(WinState::ACTIVE));
        assert!(!s.contains(WinState::MINIMIZED));
        assert_eq!(s.bits(), 1 + 2 + 8);
    }
}
