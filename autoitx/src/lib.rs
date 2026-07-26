//! AutoItX's API, in Rust, on Windows **and** macOS.
//!
//! `autoitx` drives other applications' user interfaces: keystrokes, mouse,
//! clipboard, windows, processes. The API is modeled on [AutoItX], the DLL
//! interface to AutoIt v3, so existing AutoIt automation ports over almost
//! mechanically — but unlike AutoItX, it also runs natively on macOS.
//!
//! [AutoItX]: https://www.autoitscript.com/site/autoit/
//!
//! # Platform support
//!
//! | Backend | How |
//! |---|---|
//! | Windows | FFI into `AutoItX3_x64.dll`, loaded at runtime |
//! | macOS | Native — Accessibility API, CGEvent, NSPasteboard |
//! | Linux | Planned |
//!
//! Capabilities are split three ways:
//!
//! - The **portable core** compiles everywhere.
//! - [`ext`]`::windows` and [`ext`]`::macos` hold what only one platform can
//!   do. Calling a Windows-only function in a macOS build is a **compile
//!   error**, not a runtime surprise. (Only the module matching the target
//!   platform exists in any given build, which is why these are not direct
//!   links — browse [docs.rs] to see both.)
//! - [`recipes`] expresses portable *intent* whose *mechanism* differs per
//!   platform — "wait until the app stops being busy" is one operation, even
//!   though Windows polls the cursor shape and macOS probes the Accessibility
//!   message timeout.
//!
//! [docs.rs]: https://docs.rs/autoitx
//!
//! # Two things this fixes about hand-written AutoIt code
//!
//! **Keystroke injection.** `Send` interprets `{}!+^#`, so interpolating user
//! or database data straight into a send string lets that data execute as key
//! commands. Here, `Keys::text` escapes by default and the raw form has to be
//! asked for by name.
//!
//! **Reading the screen through the clipboard.** The common idiom — put a
//! sentinel on the clipboard, copy, then check whether it changed — races with
//! anything else touching the clipboard. `recipes::read_screen_text` waits on
//! the OS clipboard sequence number instead, which cannot race.
//!
//! # Legal
//!
//! AutoIt and AutoItX are products of AutoIt Consulting Ltd. This project is
//! not affiliated with, endorsed by, or sponsored by them, and the AutoItX3
//! DLL is **not** distributed with this crate. See `NOTICE` for details on
//! obtaining it.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/autoitx/0.1.0")]

mod backend;

pub mod error;
pub mod geometry;
pub mod keys;
pub mod options;
pub mod selector;

#[cfg(any(windows, feature = "mock-loader", docsrs))]
#[cfg_attr(docsrs, doc(cfg(any(windows, feature = "mock-loader"))))]
pub mod autoit;

#[cfg(any(windows, feature = "mock-loader", docsrs))]
#[cfg_attr(docsrs, doc(cfg(any(windows, feature = "mock-loader"))))]
pub use autoit::{AutoIt, AutoItBuilder, MouseButton, Session};

pub use error::{Error, Result};
pub use geometry::{PixelCoordSpace, Point, Rect, Size};
pub use keys::Keys;
pub use options::{Options, ShowState, Speed, TitleMatchMode, WinState};
pub use selector::Selector;

/// Platform-specific capabilities.
///
/// Everything in here is gated: using an item from the wrong platform fails to
/// compile. That is deliberate — a robot that discovers mid-run that it cannot
/// read the cursor shape has already half-completed a transaction in some ERP.
pub mod ext {
    /// Capabilities with no macOS equivalent.
    #[cfg(any(windows, docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    pub mod windows {
        /// The shape of the mouse cursor, as reported by `AU3_MouseGetCursor`.
        ///
        /// Windows-only: macOS has no public API for the system-wide cursor
        /// shape (`NSCursor::currentSystemCursor` reports what *your* process
        /// would draw, not what is on screen). The portable replacement is
        /// [`recipes`]`::wait_until_idle`, which measures whether the target
        /// app is responsive rather than inferring it from the cursor.
        ///
        /// [`recipes`]: crate::recipes
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
            /// `cursor == 2 || cursor == 5`: an arrow means idle, and an I-beam
            /// means idle over a text field.
            #[must_use]
            pub const fn is_idle(self) -> bool {
                matches!(self, Self::Arrow | Self::IBeam)
            }
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
    }

    /// Capabilities with no Windows equivalent.
    #[cfg(any(target_os = "macos", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(target_os = "macos")))]
    pub mod macos {
        /// A macOS privacy permission this crate may need.
        ///
        /// Grants are keyed to a binary's path and code signature, so a plain
        /// `cargo build` can re-prompt after each rebuild, and every `cargo
        /// test` run produces a fresh hash-suffixed binary that prompts again.
        /// Granting the permission to your terminal or IDE, or ad-hoc signing
        /// with `codesign -s - --force`, avoids the churn.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Permission {
            /// Required for all window and control operations.
            Accessibility,
            /// Required only for pixel and screen-capture operations.
            ScreenRecording,
        }

        /// Whether a [`Permission`] has been granted.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum PermissionStatus {
            /// Granted; the corresponding APIs will work.
            Granted,
            /// Explicitly denied by the user.
            Denied,
            /// Never asked. Requesting it will show the system prompt.
            NotDetermined,
        }

        impl Permission {
            /// A `x-apple.systempreferences:` deep link to this permission's
            /// pane in System Settings.
            ///
            /// Embedded in permission errors so the message is actionable
            /// rather than merely accurate.
            #[must_use]
            pub const fn settings_url(self) -> &'static str {
                match self {
                    Self::Accessibility => {
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                    }
                    Self::ScreenRecording => {
                        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
                    }
                }
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn settings_urls_are_distinct_and_well_formed() {
                let a = Permission::Accessibility.settings_url();
                let s = Permission::ScreenRecording.settings_url();
                assert_ne!(a, s);
                for url in [a, s] {
                    assert!(url.starts_with("x-apple.systempreferences:"));
                }
            }
        }
    }
}

/// Portable intent, platform-specific mechanism.
///
/// Each recipe names an operation an automation actually wants ("wait until the
/// app is idle", "read the text on screen") and lets each backend implement it
/// with whatever primitive is right there. This is what keeps `#[cfg]` out of
/// business logic even though the underlying capabilities differ.
///
#[cfg(any(windows, feature = "mock-loader", docsrs))]
#[cfg_attr(docsrs, doc(cfg(any(windows, feature = "mock-loader"))))]
pub mod recipes;
