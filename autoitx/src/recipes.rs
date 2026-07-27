//! Portable intent, platform-specific mechanism.
//!
//! Some things every backend can do, but by completely different means.
//! "Wait until the application stops being busy" is one operation to the person
//! writing automation; underneath, Windows polls the mouse cursor shape and
//! macOS probes the Accessibility messaging timeout.
//!
//! Putting those behind one name is what keeps `#[cfg]` out of business logic
//! — which matters, because this crate deliberately makes platform-specific
//! calls a *compile* error. Without recipes, that choice would push the
//! conditionals into every caller.

use crate::error::{Error, Result};
use crate::{AutoIt, Point, Selector};
use std::time::Duration;

/// Clicks at an offset inside a window, wherever the window happens to be.
///
/// The alternative — an absolute screen coordinate — is what production AutoIt
/// automation usually does, and it is why that automation also has to pin the
/// screen resolution and refuse to start if it changes. Anchoring to the window
/// removes that constraint.
///
/// ```no_run
/// # use autoitx::{AutoIt, Selector, recipes};
/// # let ai = AutoIt::new()?;
/// let dialog = Selector::from("[TITLE:Acme ERP;CLASS:ui60Modal_W32]");
/// // "OK", 600 across and 420 down from the dialog's top-left corner.
/// recipes::click_in_window(&ai, &dialog, 600, 420)?;
/// # Ok::<(), autoitx::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::WindowNotFound`] if nothing matches, or whatever the click returns.
pub fn click_in_window(ai: &AutoIt, window: &Selector, dx: i32, dy: i32) -> Result<()> {
    // One session: the window must not move between measuring and clicking.
    let s = ai.session();
    let rect = s.win_get_pos(window)?;
    s.mouse_click(rect.point_at(dx, dy))
}

/// The same, for a point already computed.
///
/// # Errors
///
/// As [`click_in_window`].
pub fn click_at_offset(ai: &AutoIt, window: &Selector, offset: Point) -> Result<()> {
    click_in_window(ai, window, offset.x, offset.y)
}

/// Waits until the target application is ready for input.
///
/// On this backend that means polling the system cursor until it is an arrow or
/// an I-beam — the idiom AutoIt automation writes by hand as
/// `cursor == 2 || cursor == 5`, here with a timeout, which the hand-written
/// version invariably lacks.
///
/// The native macOS backend will implement this by probing the Accessibility
/// messaging timeout instead, which measures responsiveness directly rather
/// than inferring it from what the cursor looks like.
///
/// # Errors
///
/// [`Error::Timeout`] if the application never settles.
pub fn wait_until_idle(ai: &AutoIt, timeout: Duration) -> Result<()> {
    const POLL: Duration = Duration::from_millis(250);

    ai.wait_until("wait_until_idle", timeout, POLL, || {
        let cursor = ai.inner().mouse_get_cursor()?;
        // 2 = arrow, 5 = I-beam. Anything else — hourglass, app-starting —
        // means the application is still working.
        Ok(cursor == 2 || cursor == 5)
    })
}

/// Reads the focused field or selection by copying it, without the usual race.
///
/// The clipboard is how AutoIt automation reads a screen it cannot query: select,
/// copy, read. The hard part is knowing *when* the copy landed. The idiom in the
/// wild is to put a sentinel on the clipboard first and poll until it changes:
///
/// ```csharp
/// AutoItX.ClipPut("NO-VALUE");   // a fixed sentinel, in production code
/// // ... select and copy ...
/// if (AutoItX.ClipGet() == "NO-VALUE") { /* assume nothing was copied */ }
/// ```
///
/// That has three failure modes, and all three have been observed:
///
/// - the cell genuinely contains the sentinel, and a real value reads as empty;
/// - the copy re-writes the value that was already there, so nothing appears to
///   change and the read times out;
/// - the copy never happens at all — the keystroke went to the wrong window —
///   and the *stale* clipboard is returned as if it were this field's value.
///
/// This waits on the OS clipboard sequence number instead, which Windows bumps
/// on every write by any process. It cannot collide, it notices identical
/// rewrites, and a copy that never happened is detected rather than papered
/// over.
///
/// ```no_run
/// # use autoitx::{AutoIt, recipes, keys};
/// # use std::time::Duration;
/// # let ai = AutoIt::new()?;
/// // Select the current field and read it.
/// let value = recipes::read_screen_text(
///     &ai,
///     keys!("{END}{SHIFTDOWN}{HOME}{SHIFTUP}"),
///     Duration::from_secs(5),
/// )?;
/// # Ok::<(), autoitx::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::Timeout`] if nothing reached the clipboard — which means the copy
/// did not happen, not that the field was empty. An empty field still bumps the
/// sequence number and yields `Ok("")`.
pub fn read_screen_text(ai: &AutoIt, select: crate::Keys, timeout: Duration) -> Result<String> {
    const POLL: Duration = Duration::from_millis(100);

    // One session: a copy is only meaningful against the selection that was
    // made for it, and anything else touching the keyboard in between breaks
    // that.
    let s = ai.session();

    let before = s.clip_sequence();
    s.send(select)?;
    s.send(crate::keys!("{CTRLDOWN}c{CTRLUP}"))?;

    match before {
        Some(before) => {
            s.wait_until("read_screen_text", timeout, POLL, || {
                Ok(s.clip_sequence().is_some_and(|now| now != before))
            })?;
        }
        None => {
            // No sequence counter available. Fall back to a fixed settle,
            // which is what automation did before this existed — worse, but
            // better than pretending the copy was instantaneous.
            s.sleep(POLL * 5);
        }
    }

    s.clip_get()
}

/// Waits for a window to appear, then activates it and waits for focus.
///
/// The opening move of nearly every automation flow.
///
/// # Errors
///
/// [`Error::Timeout`] if the window never appears or never takes focus.
pub fn open_and_focus(ai: &AutoIt, window: &Selector, timeout: Duration) -> Result<()> {
    if !ai.win_wait(window, Some(timeout))? {
        return Err(Error::Timeout {
            operation: "open_and_focus (waiting for the window to appear)",
            waited: timeout,
        });
    }
    if !ai.win_wait_activate(window, Some(timeout))? {
        return Err(Error::Timeout {
            operation: "open_and_focus (waiting for focus)",
            waited: timeout,
        });
    }
    Ok(())
}
