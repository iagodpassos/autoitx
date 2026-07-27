//! The few Win32 calls AutoItX does not expose.
//!
//! Side-loaded with `libloading` rather than pulled in through the `windows`
//! crate. Two reasons: `user32.dll` is already mapped into every GUI process,
//! so opening it costs nothing; and keeping the dependency out preserves the
//! property that this crate has no link-time Windows dependency and therefore
//! cross-compiles from macOS with `cargo check` alone.

use std::sync::OnceLock;

/// Handles to the system libraries this module borrows from.
struct Win32 {
    // Kept alive so the function pointers stay valid.
    _user32: libloading::Library,
    get_clipboard_sequence_number: unsafe extern "system" fn() -> u32,
}

// SAFETY: the bound function takes no arguments, touches no shared state of
// ours, and is documented as safe to call from any thread.
unsafe impl Send for Win32 {}
// SAFETY: as above.
unsafe impl Sync for Win32 {}

static WIN32: OnceLock<Option<Win32>> = OnceLock::new();

fn win32() -> Option<&'static Win32> {
    WIN32
        .get_or_init(|| {
            // SAFETY: `user32.dll` is a system library, already loaded in any
            // process with a GUI. Opening it again only bumps its refcount.
            let user32 = unsafe { libloading::Library::new("user32.dll") }.ok()?;

            // SAFETY: `GetClipboardSequenceNumber` is documented as
            // `DWORD WINAPI GetClipboardSequenceNumber(void)`, which is exactly
            // this signature. Dereferencing erases the borrow of `user32`,
            // which is sound because both live in the same struct.
            let get_clipboard_sequence_number =
                unsafe { *user32.get(b"GetClipboardSequenceNumber\0").ok()? };

            Some(Win32 {
                _user32: user32,
                get_clipboard_sequence_number,
            })
        })
        .as_ref()
}

/// The system's clipboard sequence number, or `None` if `user32` is unavailable.
///
/// Windows bumps this counter on every clipboard write, by any process. That
/// makes it the honest answer to "did the clipboard change?", where comparing
/// contents cannot be:
///
/// - a sentinel value can collide with the real result;
/// - a value that is written *back to what it already was* is invisible;
/// - and reading contents to compare requires owning the clipboard, which
///   races with whoever else is writing.
///
/// This is what [`recipes::read_screen_text`](crate::recipes) waits on. Both
/// clipboard bugs found while porting a real RPA would have been one-liners
/// with it: each was "the command did not run", and each was diagnosed by
/// noticing the clipboard still held the command.
pub(crate) fn clipboard_sequence() -> Option<u32> {
    let w = win32()?;
    // SAFETY: no arguments; the library is kept alive by the static.
    Some(unsafe { (w.get_clipboard_sequence_number)() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_available_on_windows_and_absent_elsewhere() {
        // On Windows this must resolve; anywhere else there is no user32 and
        // `None` is the correct answer rather than a panic.
        if cfg!(windows) {
            assert!(
                clipboard_sequence().is_some(),
                "user32!GetClipboardSequenceNumber should resolve on Windows"
            );
        } else {
            assert!(clipboard_sequence().is_none());
        }
    }

    #[test]
    fn repeated_reads_do_not_panic() {
        // Exercises the OnceLock path twice, including the failure branch on
        // non-Windows, where the second call must reuse the cached `None`.
        let a = clipboard_sequence();
        let b = clipboard_sequence();
        assert_eq!(a.is_some(), b.is_some());
    }
}
