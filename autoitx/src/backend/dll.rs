//! The AutoItX DLL backend.
//!
//! Named for the DLL rather than for Windows, because it also runs on macOS
//! against the test mock — which is what makes the marshalling layer testable
//! without a Windows machine.
//!
//! Three things here are easy to get subtly wrong, so each is done in exactly
//! one place:
//!
//! - **`AU3_error` is thread-global and describes the *previous* call.** It has
//!   to be read immediately, under the same lock. The [`au3!`] macro makes
//!   forgetting impossible.
//! - **Output buffers do not report the size they needed.** AutoItX writes at
//!   most `nBufSize` wide chars including the NUL, truncates silently, and says
//!   nothing. [`call_str`] grows and retries.
//! - **AutoItX is not reentrant.** Every call takes a process-wide lock.

use crate::error::{Error, Result};
use crate::options::{Options, ShowState, Speed};
use crate::{Keys, Point, Rect, Selector};
use autoitx_sys::{AU3_INTDEFAULT, Au3, RECT};
use parking_lot::{ReentrantMutex, ReentrantMutexGuard};
use std::path::Path;
use std::time::Duration;

/// Initial capacity, in wide chars, for short values like titles and classes.
const SMALL_BUF: usize = 1024;
/// Initial capacity for values that can be large: window text, clipboard.
const LARGE_BUF: usize = 65_536;
/// Default ceiling on a returned string, in wide chars (32 MiB of UTF-16).
const DEFAULT_MAX_CHARS: usize = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// String marshalling
// ---------------------------------------------------------------------------

/// Converts to a NUL-terminated UTF-16 string.
///
/// Rejects interior NULs rather than truncating. Truncation would silently turn
/// a window title or a password into a prefix of itself, and the resulting
/// mismatch is very hard to trace back to its cause.
pub(crate) fn wide(s: &str, what: &'static str) -> Result<Vec<u16>> {
    if let Some(at) = s.bytes().position(|b| b == 0) {
        return Err(Error::InteriorNul { what, at });
    }
    Ok(s.encode_utf16().chain(std::iter::once(0)).collect())
}

/// Decodes up to the first NUL.
///
/// Lossy on purpose: applications do return lone surrogates, and failing a
/// whole automation run over one unpaired code unit helps nobody.
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// The empty string, as AutoItX wants optional string parameters.
///
/// Not null: AutoItX does not null-check its `LPCWSTR` arguments.
static EMPTY_WIDE: [u16; 1] = [0];

// ---------------------------------------------------------------------------
// The backend
// ---------------------------------------------------------------------------

/// Everything shared between clones of a handle to the automation session.
pub(crate) struct Inner {
    au3: Au3,
    /// Serialises access. Reentrant so a [`Session`](crate::Session) can hold
    /// it across many calls without the per-call locks deadlocking.
    lock: ReentrantMutex<()>,
    options: Options,
    max_chars: usize,
}

// SAFETY: every access to `au3` goes through `lock`, and AutoItX's global state
// (its option table and per-thread error flag) is only touched while held.
unsafe impl Send for Inner {}
// SAFETY: as above — `&Inner` only exposes methods that take the lock first.
unsafe impl Sync for Inner {}

/// Calls an AU3 function under the lock and reads its error flag.
///
/// Returns `(return_value, error_code)`. The two must be read together: the
/// flag is thread-global and the next call overwrites it.
macro_rules! au3 {
    ($inner:expr, $name:ident ( $($arg:expr),* $(,)? )) => {{
        let inner = $inner;
        let _guard = inner.lock.lock();

        #[cfg(feature = "tracing")]
        let _span = ::tracing::trace_span!(stringify!($name)).entered();

        // SAFETY: `inner.au3` was bound from a library asserted to be an
        // AutoItX3 DLL, the arguments match the declared signature, and the
        // lock makes the call non-reentrant as AutoItX requires.
        let ret = unsafe { (inner.au3.$name)($($arg),*) };
        // SAFETY: same library; takes no arguments. Read immediately so it
        // still describes the call above.
        let code = unsafe { (inner.au3.AU3_error)() };
        (ret, code)
    }};
}

impl Inner {
    pub(crate) fn load(
        dll: Option<&Path>,
        options: Options,
        max_chars: Option<usize>,
    ) -> Result<Self> {
        // SAFETY: `Au3::load` is documented to require an AutoItX3 DLL. The
        // search order only ever produces paths the user named, or the
        // conventional install locations.
        let au3 = unsafe { Au3::load(dll) }?;
        Ok(Self {
            au3,
            lock: ReentrantMutex::new(()),
            options,
            max_chars: max_chars.unwrap_or(DEFAULT_MAX_CHARS),
        })
    }

    pub(crate) const fn options(&self) -> &Options {
        &self.options
    }

    /// Holds the lock for a run of calls.
    pub(crate) fn lock(&self) -> ReentrantMutexGuard<'_, ()> {
        self.lock.lock()
    }

    /// Runs a string-returning call, growing the buffer until it fits.
    ///
    /// AutoItX gives no "required size" signal, so a result that fills the
    /// buffer to the brim is treated as possibly truncated and retried larger.
    /// The cost of a false positive is one extra call; the cost of a false
    /// negative is silently losing the tail of a value.
    fn call_str(
        &self,
        func: &'static str,
        initial: usize,
        mut call: impl FnMut(*mut u16, i32) -> i32,
    ) -> Result<String> {
        let mut cap = initial;
        loop {
            let mut buf = vec![0u16; cap];
            let code = call(buf.as_mut_ptr(), cap as i32);

            let len = buf.iter().position(|&c| c == 0).unwrap_or(cap);
            if len + 1 < cap {
                let _ = code;
                return Ok(from_wide(&buf));
            }

            if cap >= self.max_chars {
                return Err(Error::StringTooLarge {
                    func,
                    limit: self.max_chars,
                });
            }
            cap = cap.saturating_mul(4).min(self.max_chars);
        }
    }

    /// A selector, as the wide string AutoItX expects.
    fn sel(&self, s: &Selector) -> Result<Vec<u16>> {
        wide(&s.to_string(), "selector")
    }
}

/// Seconds for AutoIt's timeout parameters; `None` means wait forever.
fn timeout_secs(t: Option<Duration>) -> i32 {
    // AutoIt reads 0 as "no timeout". Anything under a second would otherwise
    // round to 0 and wait forever, which is the opposite of what was asked, so
    // sub-second timeouts are raised to 1.
    match t {
        None => 0,
        Some(d) => i32::try_from(d.as_secs()).unwrap_or(i32::MAX).max(1),
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

impl Inner {
    pub(crate) fn send(&self, keys: &Keys) -> Result<()> {
        let w = wide(keys.as_str(), "keys")?;
        // Mode 0: interpret `{}!+^#`. Mode 1 (raw) is never what callers want
        // here — `Keys::text` has already escaped anything that needed it.
        au3!(self, AU3_Send(w.as_ptr(), 0));
        Ok(())
    }

    pub(crate) fn clip_get(&self) -> Result<String> {
        // AutoItX sets the error flag when the clipboard holds no text, and
        // returns "". Mapping that to Ok("") rather than Err is deliberate: it
        // is what every existing call site expects, and an empty clipboard is
        // an ordinary state, not a failure.
        self.call_str("AU3_ClipGet", LARGE_BUF, |buf, cap| {
            let (_, code) = au3!(self, AU3_ClipGet(buf, cap));
            code
        })
    }

    pub(crate) fn clip_put(&self, s: &str) -> Result<()> {
        let w = wide(s, "clipboard text")?;
        au3!(self, AU3_ClipPut(w.as_ptr()));
        Ok(())
    }

    pub(crate) fn mouse_click(
        &self,
        button: &str,
        p: Point,
        clicks: u32,
        speed: Option<Speed>,
    ) -> Result<()> {
        let b = wide(button, "mouse button")?;
        let spd = speed.map_or(AU3_INTDEFAULT, Speed::get);
        // Unlike `WinGetPos`, `MouseClick` *is* documented as returning 1 on
        // success and 0 on failure — and only when the button name is not one
        // AutoIt knows. So the return is the right signal here, but the error
        // reported carries the flag too, so a surprise is diagnosable from one
        // failure rather than two.
        let (ok, code) = au3!(
            self,
            AU3_MouseClick(b.as_ptr(), p.x, p.y, clicks as i32, spd)
        );
        if ok == 0 {
            return Err(Error::AutoItFailed {
                func: "AU3_MouseClick",
                code,
            });
        }
        Ok(())
    }

    pub(crate) fn win_exists(&self, s: &Selector) -> Result<bool> {
        let w = self.sel(s)?;
        let (found, _) = au3!(self, AU3_WinExists(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(found != 0)
    }

    pub(crate) fn win_active(&self, s: &Selector) -> Result<bool> {
        let w = self.sel(s)?;
        let (active, _) = au3!(self, AU3_WinActive(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(active != 0)
    }

    pub(crate) fn win_activate(&self, s: &Selector) -> Result<()> {
        let w = self.sel(s)?;
        au3!(self, AU3_WinActivate(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(())
    }

    pub(crate) fn win_wait_active(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinWaitActive(w.as_ptr(), EMPTY_WIDE.as_ptr(), timeout_secs(t))
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_wait(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinWait(w.as_ptr(), EMPTY_WIDE.as_ptr(), timeout_secs(t))
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_wait_close(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinWaitClose(w.as_ptr(), EMPTY_WIDE.as_ptr(), timeout_secs(t))
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_close(&self, s: &Selector) -> Result<()> {
        let w = self.sel(s)?;
        au3!(self, AU3_WinClose(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(())
    }

    pub(crate) fn win_get_process(&self, s: &Selector) -> Result<u32> {
        let w = self.sel(s)?;
        let (pid, _) = au3!(self, AU3_WinGetProcess(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(pid)
    }

    pub(crate) fn win_set_state(&self, s: &Selector, state: ShowState) -> Result<()> {
        let w = self.sel(s)?;
        au3!(
            self,
            AU3_WinSetState(w.as_ptr(), EMPTY_WIDE.as_ptr(), state as i32)
        );
        Ok(())
    }

    pub(crate) fn win_get_pos(&self, s: &Selector) -> Result<Rect> {
        let w = self.sel(s)?;
        let mut rect = RECT::default();
        // The error flag, not the integer return, is what says whether the
        // window was found.
        //
        // AutoIt documents `WinGetPos` as reporting failure through @error; its
        // DLL form fills the RECT and leaves the `int` return unspecified. An
        // earlier version of this treated a 0 return as "not found", and it
        // rejected windows that plainly existed — `win_get_title` would answer
        // for the very same selector a line earlier.
        //
        // The rule for this ABI: functions that fill an out-parameter report
        // through the error flag; only the ones documented as returning 1/0
        // (`WinExists`, `MouseClick`, the `WinWait*` family) have a meaningful
        // return.
        let (_, code) = au3!(
            self,
            AU3_WinGetPos(w.as_ptr(), EMPTY_WIDE.as_ptr(), &raw mut rect)
        );
        if code != 0 {
            return Err(Error::window_not_found(s));
        }
        Ok(Rect::from(rect))
    }

    pub(crate) fn win_get_title(&self, s: &Selector) -> Result<String> {
        let w = self.sel(s)?;
        self.call_str("AU3_WinGetTitle", SMALL_BUF, |buf, cap| {
            let (_, code) = au3!(
                self,
                AU3_WinGetTitle(w.as_ptr(), EMPTY_WIDE.as_ptr(), buf, cap)
            );
            code
        })
    }

    pub(crate) fn run(&self, command: &str, working_dir: Option<&str>) -> Result<u32> {
        let cmd = wide(command, "command")?;
        let dir = match working_dir {
            Some(d) => wide(d, "working directory")?,
            None => vec![0u16],
        };
        let (pid, code) = au3!(self, AU3_Run(cmd.as_ptr(), dir.as_ptr(), AU3_INTDEFAULT));
        if pid == 0 {
            return Err(Error::AutoItFailed {
                func: "AU3_Run",
                code,
            });
        }
        Ok(pid as u32)
    }

    pub(crate) fn process_close(&self, name_or_pid: &str) -> Result<()> {
        let w = wide(name_or_pid, "process")?;
        au3!(self, AU3_ProcessClose(w.as_ptr()));
        Ok(())
    }

    pub(crate) fn mouse_get_cursor(&self) -> Result<i32> {
        let (code, _) = au3!(self, AU3_MouseGetCursor());
        Ok(code)
    }

    pub(crate) fn sleep(&self, d: Duration) {
        // Deliberately not an AU3_Sleep call: that would hold the lock for the
        // whole duration, blocking every other thread's automation for no
        // reason. AutoIt's own sleep offers nothing over the OS one.
        std::thread::sleep(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_rejects_interior_nul_rather_than_truncating() {
        let err = wide("abc\0def", "test").unwrap_err();
        match err {
            Error::InteriorNul { at, .. } => assert_eq!(at, 3),
            other => panic!("expected InteriorNul, got {other:?}"),
        }
    }

    #[test]
    fn wide_round_trips_non_ascii() {
        let w = wide("Order Entry", "test").unwrap();
        assert_eq!(*w.last().unwrap(), 0, "must be NUL-terminated");
        assert_eq!(from_wide(&w), "Order Entry");
    }

    #[test]
    fn from_wide_stops_at_the_first_nul() {
        let buf = [b'h' as u16, b'i' as u16, 0, b'x' as u16];
        assert_eq!(from_wide(&buf), "hi");
    }

    #[test]
    fn timeouts_convert_to_autoits_seconds() {
        assert_eq!(timeout_secs(None), 0, "no timeout means wait forever");
        assert_eq!(timeout_secs(Some(Duration::from_secs(30))), 30);
        // A sub-second timeout must not round down to 0, which AutoIt reads as
        // "wait forever" — the exact opposite of the caller's intent.
        assert_eq!(timeout_secs(Some(Duration::from_millis(1))), 1);
        assert_eq!(timeout_secs(Some(Duration::ZERO)), 1);
    }
}
