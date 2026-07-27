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

// Re-exported as `ext::windows`, which is how it should be named in code. It is
// `pub` here only because a `pub use` cannot re-export a private module.
#[cfg(any(windows, docsrs))]
#[doc(hidden)]
pub mod ext_windows;

// Controls are addressed by Win32 class-and-instance, and every method that
// takes one lives on the DLL backend. Gating the type as well as the methods
// means a macOS build fails on the `Control` rather than on the call — one
// error at the source instead of one per call site.
#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub mod control;
pub mod error;
pub mod geometry;
pub mod keys;
pub mod options;
pub mod selector;

#[cfg(any(windows, target_os = "macos", feature = "mock-loader", docsrs))]
pub mod autoit;

#[cfg(any(windows, target_os = "macos", feature = "mock-loader", docsrs))]
pub use autoit::{AutoIt, AutoItBuilder, MouseButton, Session};

#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub use control::Control;
pub use error::{Error, Result};
pub use geometry::{PixelCoordSpace, Point, Rect, Size};
pub use keys::Keys;
pub use options::{KeyMap, Options, ShowState, Speed, TitleMatchMode, WinState};
pub use selector::Selector;

/// Platform-specific capabilities.
///
/// Everything in here is gated: using an item from the wrong platform fails to
/// compile. That is deliberate — a robot that discovers mid-run that it cannot
/// read the cursor shape has already half-completed a transaction in some ERP.
pub mod ext {
    /// Capabilities with no macOS equivalent.
    ///
    /// Defined in `crate::ext_windows`; re-exported here so the two platform
    /// modules sit side by side.
    #[cfg(any(windows, docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    pub use crate::ext_windows as windows;

    /// Capabilities with no Windows equivalent.
    ///
    /// Defined in `crate::backend::macos`; re-exported here so the two
    /// platform modules sit side by side.
    #[cfg(target_os = "macos")]
    #[cfg_attr(docsrs, doc(cfg(target_os = "macos")))]
    pub mod macos {
        pub use crate::backend::macos::permissions::{
            Permission, PermissionStatus, check, request,
        };

        /// Whether an application is answering, rather than beachballing.
        ///
        /// The macOS half of
        /// [`recipes::wait_until_idle`](crate::recipes::wait_until_idle), and
        /// exposed on its own because it answers a question Windows cannot:
        /// *this* application is busy, as opposed to "the pointer looks busy".
        ///
        /// Asks the application for an accessibility attribute with `timeout`
        /// to answer in. A process that is stopped, deadlocked, or churning
        /// inside a synchronous call does not answer and reads as `false`.
        ///
        /// ```no_run
        /// # use std::time::Duration;
        /// use autoitx::ext::macos;
        ///
        /// # let pid = 0;
        /// if !macos::is_app_responsive(pid, Duration::from_millis(500)) {
        ///     // Do not send keystrokes into an application that is not reading.
        /// }
        /// ```
        #[must_use]
        pub fn is_app_responsive(pid: i32, timeout: std::time::Duration) -> bool {
            crate::backend::macos::window::is_app_responsive(pid, timeout)
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
#[cfg(any(windows, target_os = "macos", feature = "mock-loader", docsrs))]
pub mod recipes;
