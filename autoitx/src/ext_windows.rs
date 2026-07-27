//! Capabilities with no macOS equivalent.
//!
//! Everything here is gated on Windows: using one of these in a macOS build is
//! a compile error, not a runtime surprise. That is deliberate — a robot that
//! discovers mid-run it cannot map a network drive has usually already
//! half-completed something.
//!
//! Where a macOS equivalent exists in spirit but not in shape, it lives in
//! [`recipes`](crate::recipes) instead, so portable automation can express the
//! intent without naming the mechanism.

use crate::error::Result;
use crate::{AutoIt, Point, Selector};

/// The shape of the mouse cursor, as reported by `AU3_MouseGetCursor`.
///
/// Windows-only: macOS has no public API for the system-wide cursor shape
/// (`NSCursor::currentSystemCursor` reports what *your* process would draw, not
/// what is on screen). The portable replacement is
/// [`recipes::wait_until_idle`](crate::recipes::wait_until_idle), which measures
/// whether the target app is responsive rather than inferring it from the
/// cursor.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseCursor {
    /// Unrecognized, or an application-defined cursor.
    Unknown = 0,
    /// Arrow with an hourglass — the app is starting something.
    AppStarting = 1,
    /// The ordinary arrow.
    Arrow = 2,
    /// Crosshair.
    Cross = 3,
    /// Arrow with a question mark.
    Help = 4,
    /// Text insertion caret.
    IBeam = 5,
    /// Icon drag cursor.
    Icon = 6,
    /// The "no drop" circle-slash.
    No = 7,
    /// Generic resize.
    Size = 8,
    /// Four-way move.
    SizeAll = 9,
    /// Diagonal resize, northeast/southwest.
    SizeNeSw = 10,
    /// Vertical resize.
    SizeNs = 11,
    /// Diagonal resize, northwest/southeast.
    SizeNwSe = 12,
    /// Horizontal resize.
    SizeWe = 13,
    /// Up arrow.
    UpArrow = 14,
    /// The hourglass / spinner — the app is busy.
    Wait = 15,
}

impl MouseCursor {
    /// Maps a raw `AU3_MouseGetCursor` code, tolerating unknown values.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code {
            1 => Self::AppStarting,
            2 => Self::Arrow,
            3 => Self::Cross,
            4 => Self::Help,
            5 => Self::IBeam,
            6 => Self::Icon,
            7 => Self::No,
            8 => Self::Size,
            9 => Self::SizeAll,
            10 => Self::SizeNeSw,
            11 => Self::SizeNs,
            12 => Self::SizeNwSe,
            13 => Self::SizeWe,
            14 => Self::UpArrow,
            15 => Self::Wait,
            _ => Self::Unknown,
        }
    }

    /// Whether the cursor indicates the application is ready for input.
    ///
    /// Names the idiom that AutoIt automation writes by hand as
    /// `cursor == 2 || cursor == 5`: an arrow means idle, and an I-beam means
    /// idle over a text field.
    #[must_use]
    pub const fn is_idle(self) -> bool {
        matches!(self, Self::Arrow | Self::IBeam)
    }
}

// ---------------------------------------------------------------------------
// Window extras
// ---------------------------------------------------------------------------

/// Sets a window's opacity, 0 (invisible) to 255 (opaque).
///
/// Windows-only. macOS exposes window opacity through the owning application,
/// not to outside processes.
pub fn win_set_trans(ai: &AutoIt, window: &Selector, alpha: u8) -> Result<bool> {
    ai.inner().win_set_trans(window, alpha)
}

/// Picks an item from a window's menu bar, by text.
///
/// Up to eight levels deep: `&["&File", "&Recent", "report.xls"]`. Windows-only
/// because it drives a Win32 menu; macOS menus live in the accessibility tree
/// and are reached differently.
///
/// Returns whether the item was found.
pub fn win_menu_select_item(ai: &AutoIt, window: &Selector, path: &[&str]) -> Result<bool> {
    ai.inner().win_menu_select_item(window, path)
}

/// Reads one part of a window's status bar.
///
/// `part` is 1-based. Status bars are a Win32 common control with no macOS
/// counterpart.
pub fn statusbar_get_text(ai: &AutoIt, window: &Selector, part: u32) -> Result<String> {
    ai.inner().statusbar_get_text(window, part)
}

/// Where the text caret is, in screen coordinates.
///
/// # Errors
///
/// [`Error::AutoItFailed`](crate::Error::AutoItFailed) if no window has a caret.
pub fn caret_pos(ai: &AutoIt) -> Result<Point> {
    ai.inner().win_get_caret_pos()
}

// ---------------------------------------------------------------------------
// Running as someone else, and shutting down
// ---------------------------------------------------------------------------

/// Runs a program as another user.
///
/// The user's profile is loaded, which is what makes their mapped drives and
/// per-user settings behave as expected. Set `wait` to block until it exits,
/// in which case the exit code is returned.
pub fn run_as(
    ai: &AutoIt,
    user: &str,
    domain: &str,
    password: &str,
    command: &str,
    working_dir: Option<&str>,
    wait: bool,
) -> Result<i32> {
    ai.inner()
        .run_as(user, domain, password, command, working_dir, wait)
}

/// Logs off, reboots, or powers down.
///
/// `flags` is a bitmask: 0 log off, 1 shut down, 2 reboot, 4 force, 8 power
/// down. So `1 | 8` powers the machine off.
///
/// Deliberately not wrapped in a friendlier enum: an automation that shuts a
/// machine down should be spelling out exactly what it means.
pub fn shutdown(ai: &AutoIt, flags: i32) -> Result<bool> {
    ai.inner().shutdown(flags)
}

// ---------------------------------------------------------------------------
// Mapped network drives
// ---------------------------------------------------------------------------

/// Maps a network share to a drive letter.
///
/// `device` is `"X:"`, or `"*"` to take the next free letter — in which case
/// the letter actually assigned is returned. Pass empty strings for `user` and
/// `password` to connect as the current user.
///
/// No macOS equivalent: drive letters are a Windows concept, and `mount_smbfs`
/// is a different model rather than a different spelling.
pub fn drive_map_add(
    ai: &AutoIt,
    device: &str,
    share: &str,
    user: &str,
    password: &str,
) -> Result<String> {
    ai.inner().drive_map_add(device, share, user, password)
}

/// Disconnects a mapped drive.
pub fn drive_map_del(ai: &AutoIt, device: &str) -> Result<bool> {
    ai.inner().drive_map_del(device)
}

/// The share a drive letter is mapped to, or an empty string.
pub fn drive_map_get(ai: &AutoIt, device: &str) -> Result<String> {
    ai.inner().drive_map_get(device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_arrow_or_ibeam_only() {
        assert!(MouseCursor::from_code(2).is_idle());
        assert!(MouseCursor::from_code(5).is_idle());
        assert!(!MouseCursor::from_code(15).is_idle()); // Wait
        assert!(!MouseCursor::from_code(1).is_idle()); // AppStarting
    }

    #[test]
    fn unknown_codes_do_not_panic() {
        assert_eq!(MouseCursor::from_code(-1), MouseCursor::Unknown);
        assert_eq!(MouseCursor::from_code(9999), MouseCursor::Unknown);
        assert!(!MouseCursor::from_code(9999).is_idle());
    }
}
