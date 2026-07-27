//! What can go wrong.

use crate::Selector;
use std::time::Duration;

/// The result type used throughout this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// An automation failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The AutoItX3 DLL could not be found or loaded.
    ///
    /// The inner error lists every path tried — see
    /// [`autoitx_sys::LoadError`].
    #[error(transparent)]
    Load(#[from] autoitx_sys::LoadError),

    /// An AutoIt call set its error flag.
    ///
    /// AutoIt's error reporting is a single thread-global integer with no
    /// message, so this carries the function name to make it locatable.
    #[error("{func} failed (AutoIt error code {code})")]
    AutoItFailed {
        /// The AU3 function that failed.
        func: &'static str,
        /// AutoIt's error code.
        code: i32,
    },

    /// No window matched.
    #[error("no window matched {selector}")]
    WindowNotFound {
        /// The selector that matched nothing.
        selector: Box<Selector>,
    },

    /// The window was found, but it has no such control.
    ///
    /// Named separately from [`WindowNotFound`](Self::WindowNotFound) because
    /// the two have different fixes: a wrong window selector versus a wrong
    /// control identifier.
    #[error("window {selector} has no control matching {control}")]
    ControlNotFound {
        /// The window that was found.
        selector: Box<Selector>,
        /// The control identifier that matched nothing.
        control: crate::Control,
    },

    /// An operation did not complete in time.
    #[error("{operation} timed out after {waited:?}")]
    Timeout {
        /// What was being waited for.
        operation: &'static str,
        /// How long it waited.
        waited: Duration,
    },

    /// A selector could not be built or parsed.
    #[error(transparent)]
    Selector(#[from] crate::selector::SelectorError),

    /// A key sequence could not be parsed.
    #[error(transparent)]
    Keys(#[from] crate::keys::KeyParseError),

    /// A string argument contained an interior NUL.
    ///
    /// Win32 strings are NUL-terminated, so this would silently truncate.
    /// Truncating a window title or a password is worse than failing.
    #[error("{what} contains an interior NUL byte at index {at}, which Win32 strings cannot carry")]
    InteriorNul {
        /// Which argument was at fault.
        what: &'static str,
        /// Byte offset of the NUL.
        at: usize,
    },

    /// A returned string exceeded the configured limit.
    ///
    /// AutoIt never reports how large a buffer it needed, so the safe layer
    /// grows and retries. This is the ceiling on that growth.
    #[error("{func} returned more than {limit} bytes of text")]
    StringTooLarge {
        /// The AU3 function that produced it.
        func: &'static str,
        /// The configured ceiling.
        limit: usize,
    },

    /// The process-wide automation lock could not be acquired.
    ///
    /// Another process holds it. Automation cannot safely interleave — two
    /// robots sending keystrokes at once fight over focus — so this is a
    /// failure, not something to work around.
    #[error("timed out after {waited:?} waiting for the global automation mutex {name:?}")]
    GlobalMutexTimeout {
        /// The named mutex.
        name: String,
        /// How long it waited.
        waited: Duration,
    },

    /// A macOS privacy permission has not been granted.
    ///
    /// Carries a copy-pasteable hint rather than only the fact, because without
    /// the Accessibility grant every AX call fails in a way that reads exactly
    /// like "window not found" — and chasing that costs an hour.
    #[cfg(target_os = "macos")]
    #[cfg_attr(docsrs, doc(cfg(target_os = "macos")))]
    #[error("{permission:?} permission not granted — {hint}")]
    PermissionDenied {
        /// Which permission is missing.
        permission: crate::ext::macos::Permission,
        /// What to do about it, with the System Settings deep link.
        hint: String,
    },

    /// An underlying I/O failure, e.g. launching a program.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Builds a [`Error::WindowNotFound`].
    #[must_use]
    pub fn window_not_found(selector: &Selector) -> Self {
        Self::WindowNotFound {
            selector: Box::new(selector.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_not_found_names_the_selector() {
        let sel = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]");
        let msg = Error::window_not_found(&sel).to_string();
        assert!(msg.contains("Chrome_WidgetWin_1"), "{msg}");
        assert!(msg.contains("Acme Invoices"), "{msg}");
    }

    #[test]
    fn timeout_reports_what_and_how_long() {
        let msg = Error::Timeout {
            operation: "win_wait_active",
            waited: Duration::from_secs(30),
        }
        .to_string();
        assert!(msg.contains("win_wait_active"), "{msg}");
        assert!(msg.contains("30s"), "{msg}");
    }

    #[test]
    fn error_does_not_grow_unboundedly() {
        // Not a performance claim — every operation here is an OS round trip
        // measured in milliseconds, so a few words in the Result are noise.
        // The point is to notice if someone inlines a collection into a
        // variant. `Selector` is boxed for that reason; the current 56 bytes
        // come from `autoitx_sys::LoadError`, which is cold and carries the
        // full list of searched paths.
        const CEILING: usize = 64;
        assert!(
            size_of::<Error>() <= CEILING,
            "Error grew to {} bytes (ceiling {CEILING}) — did a variant gain a \
             collection field? Box it.",
            size_of::<Error>()
        );
    }
}
