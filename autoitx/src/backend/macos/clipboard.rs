//! The pasteboard, and its change counter.
//!
//! macOS gives here for free what Windows needed a side-loaded `user32` call
//! to provide: `NSPasteboard.changeCount` increments on every write by any
//! process, which is what makes
//! [`recipes::read_screen_text`](crate::recipes) race-free rather than
//! merely careful.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// The general pasteboard — the one Cmd-C and Cmd-V use.
#[allow(dead_code, reason = "wired into AutoIt when the backend is selected")]
fn general() -> objc2::rc::Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

/// Reads the pasteboard as text.
///
/// An empty or non-text pasteboard yields `Ok("")` rather than an error, which
/// matches the Windows backend and, more importantly, matches what callers
/// mean: an empty clipboard is an ordinary state.
#[allow(dead_code, reason = "wired into AutoIt when the backend is selected")]
pub(crate) fn get() -> crate::Result<String> {
    let pb = general();
    // SAFETY: reading a string of the standard text type; `None` when the
    // pasteboard holds no text representation.
    let text = unsafe { pb.stringForType(NSPasteboardTypeString) };
    Ok(text.map(|s| s.to_string()).unwrap_or_default())
}

/// Replaces the pasteboard contents with text.
#[allow(dead_code, reason = "wired into AutoIt when the backend is selected")]
pub(crate) fn put(s: &str) -> crate::Result<()> {
    let pb = general();
    // `clearContents` is required before writing: without it the write is
    // rejected, because a pasteboard write must follow a declared change.
    // SAFETY: standard pasteboard write sequence.
    unsafe {
        pb.clearContents();
        let ns = NSString::from_str(s);
        pb.setString_forType(&ns, NSPasteboardTypeString);
    }
    Ok(())
}

/// The pasteboard's change count.
///
/// Increments on every write, by any process. The macOS answer to Windows'
/// `GetClipboardSequenceNumber`, and the reason a copy that produces the same
/// text as last time is still detectable.
///
/// `NSInteger` is 64-bit here and the crate's portable API is `u32`, so this
/// truncates. That is fine for its only purpose — comparing two readings taken
/// moments apart — and a wrap would need 4 billion clipboard writes between
/// them.
#[allow(dead_code, reason = "wired into AutoIt when the backend is selected")]
pub(crate) fn sequence() -> Option<u32> {
    let pb = general();
    let count = pb.changeCount();
    Some(count as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the clipboard tests: they share one system pasteboard, and
    /// Cargo runs tests in parallel threads.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores whatever was on the pasteboard when the test started.
    ///
    /// These tests run on a developer's machine against the real clipboard.
    /// Leaving test junk in someone's paste buffer is rude, and `Drop` puts it
    /// back even when an assertion fails.
    struct Preserve {
        original: String,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl Preserve {
        fn new() -> Self {
            let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Self {
                original: get().unwrap_or_default(),
                _guard: guard,
            }
        }
    }

    impl Drop for Preserve {
        fn drop(&mut self) {
            let _ = put(&self.original);
        }
    }

    #[test]
    fn text_round_trips_including_non_ascii() {
        let _p = Preserve::new();
        for s in ["", "plain", "Ünïcödé ãõç — 1.234,56", "ção ãõç", "多字节"] {
            put(s).unwrap();
            assert_eq!(get().unwrap(), s, "round trip failed for {s:?}");
        }
    }

    #[test]
    fn the_change_count_moves_on_every_write() {
        let _p = Preserve::new();
        let before = sequence().expect("macOS always has a change count");

        put("first").unwrap();
        let after_first = sequence().unwrap();
        assert_ne!(after_first, before, "a write must bump the counter");

        // The point of the counter, and the reason comparing contents cannot
        // replace it: writing the *same* text again is invisible to a content
        // comparison and plainly visible here.
        put("first").unwrap();
        let after_same = sequence().unwrap();
        assert_ne!(
            after_same, after_first,
            "rewriting identical text must still bump the counter"
        );
    }

    #[test]
    fn reading_does_not_bump_the_counter() {
        let _p = Preserve::new();
        put("stable").unwrap();
        let before = sequence().unwrap();
        for _ in 0..3 {
            let _ = get().unwrap();
        }
        assert_eq!(
            sequence().unwrap(),
            before,
            "reading must not count as a change"
        );
    }
}
