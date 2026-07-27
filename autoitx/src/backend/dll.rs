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
use crate::options::{Options, ShowState, Speed, WinState};
use crate::{Keys, Point, Rect, Selector, Size};
use autoitx_sys::{AU3_INTDEFAULT, Au3, POINT, RECT};
use parking_lot::{ReentrantMutex, ReentrantMutexGuard};
use std::path::Path;
use std::time::Duration;

/// Initial capacity, in wide chars, for short values like titles and classes.
const SMALL_BUF: usize = 1024;
/// Initial capacity for values that can be large: window text, clipboard.
const LARGE_BUF: usize = 65_536;
/// Default ceiling on a returned string, in wide chars (32 MiB of UTF-16).
const DEFAULT_MAX_CHARS: usize = 16 * 1024 * 1024;

/// What `AU3_WinGetProcess` returns when no window matched: `(DWORD)-1`.
///
/// Not 0, which is what one would guess and what an earlier version of this
/// checked for. Established by calling the real DLL against a window that does
/// not exist.
const INVALID_PID: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// How this ABI reports failure
// ---------------------------------------------------------------------------
//
// There is no single convention, and the documentation on autoitscript.com
// describes AutoIt's *script* functions rather than the DLL's. The table below
// was measured by calling the real DLL against a window that exists and one
// that does not, and it is the authority for the wrappers in this file:
//
//   function                existing        missing         signal
//   ---------------------------------------------------------------------
//   WinExists               ret 1           ret 0           return
//   WinActive               ret 1           ret 0           return
//   WinActivate             ret 1           ret 0           return
//   WinSetState             ret 1           ret 0           return
//   WinGetProcess           ret <pid>       ret 0xFFFFFFFF  return, but the
//                                                           sentinel is -1
//   WinGetState             ret <bits>      err 1           error flag
//   WinGetPos               ret 0, err 0    ret 1, err 1    error flag
//   WinGetTitle             text, err 0     "", err 0       NONE
//   WinGetText              text, err 0     "", err 0       NONE
//   WinGetClassList         text, err 0     "", err 1       error flag
//   ProcessExists           ret <pid>       ret 0           return
//   PixelGetColor           ret <rgb>       ret 0xFFFFFF    NONE
//
// Three of these deserve naming:
//
// `WinGetPos`'s return is 0 on *success* and 1 on failure — inverted against
// every other function here. Reading it as a status is not merely unreliable,
// it is backwards.
//
// `WinGetTitle` and `WinGetText` report **nothing**. A missing window and a
// window with no title are both an empty string with a clear error flag, and
// there is no way to tell them apart. `WinGetClassList`, sitting right next to
// them and shaped identically, *does* set the flag — so the inconsistency is
// AutoItX's, not a misreading.
//
// `PixelGetColor` also reports nothing: a coordinate far off-screen returns
// 0xFFFFFF, indistinguishable from a genuinely white pixel.

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

    /// The raw function table. See [`AutoIt::raw`](crate::AutoIt::raw).
    pub(crate) const fn raw(&self) -> &Au3 {
        &self.au3
    }

    /// Reads AutoIt's error flag without making a call.
    pub(crate) fn raw_error(&self) -> i32 {
        let (code, _) = au3!(self, AU3_error());
        code
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

    pub(crate) fn win_activate(&self, s: &Selector) -> Result<bool> {
        let w = self.sel(s)?;
        // Measured, not assumed: the DLL returns 1/0 here. AutoIt's *script*
        // documentation says `WinActivate` returns the window handle, which
        // would make "non-zero means success" unsafe — a handle whose low 32
        // bits are zero would read as failure. The DLL form does not do that.
        let (ok, _) = au3!(self, AU3_WinActivate(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(ok != 0)
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
        // Failure is `(DWORD)-1`, not 0 — measured against a real desktop.
        //
        // This one had teeth: a caller checking `pid != 0` before killing the
        // process would sail past 4294967295 and try to terminate it. Returning
        // the sentinel as if it were a process id is not an option.
        if pid == INVALID_PID {
            return Err(Error::window_not_found(s));
        }
        Ok(pid)
    }

    pub(crate) fn win_set_state(&self, s: &Selector, state: ShowState) -> Result<bool> {
        let w = self.sel(s)?;
        // Measured: 1 when the window was found, 0 when it was not.
        let (ok, _) = au3!(
            self,
            AU3_WinSetState(w.as_ptr(), EMPTY_WIDE.as_ptr(), state as i32)
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_get_state(&self, s: &Selector) -> Result<WinState> {
        let w = self.sel(s)?;
        // Measured: reports through the error flag, like `WinGetPos` — the
        // return is the state bits, and 0 is a legitimate value.
        let (bits, code) = au3!(self, AU3_WinGetState(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        if code != 0 {
            return Err(Error::window_not_found(s));
        }
        Ok(WinState::from_bits_truncate(bits))
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

    /// Resolves a selector to the handle of the window it currently matches.
    ///
    /// The point is to freeze an identity. `[ACTIVE]` and a title prefix both
    /// name "whatever matches right now", which is fine for a single call and
    /// wrong for a sequence — by the second call, the answer may be a different
    /// window.
    pub(crate) fn win_get_handle(&self, s: &Selector) -> Result<u64> {
        let w = self.sel(s)?;
        let (h, _) = au3!(self, AU3_WinGetHandle(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        if h.is_null() {
            return Err(Error::window_not_found(s));
        }
        Ok(h as usize as u64)
    }

    pub(crate) fn win_get_text(&self, s: &Selector) -> Result<String> {
        let w = self.sel(s)?;
        self.call_str("AU3_WinGetText", LARGE_BUF, |buf, cap| {
            let (_, code) = au3!(
                self,
                AU3_WinGetText(w.as_ptr(), EMPTY_WIDE.as_ptr(), buf, cap)
            );
            code
        })
    }

    pub(crate) fn win_get_class_list(&self, s: &Selector) -> Result<Vec<String>> {
        let w = self.sel(s)?;
        // Unlike its two neighbours, this one *does* set the error flag when
        // nothing matched — checked separately because `call_str` deliberately
        // ignores the flag (an empty clipboard sets it, and that is not an
        // error).
        let joined = self.call_str("AU3_WinGetClassList", LARGE_BUF, |buf, cap| {
            let (_, code) = au3!(
                self,
                AU3_WinGetClassList(w.as_ptr(), EMPTY_WIDE.as_ptr(), buf, cap)
            );
            code
        })?;
        if joined.is_empty() {
            return Err(Error::window_not_found(s));
        }
        // Newline-separated, and real windows produce a lot of them — a modern
        // Notepad reports several hundred characters' worth.
        Ok(joined.lines().map(str::to_owned).collect())
    }

    pub(crate) fn process_id(&self, name: &str) -> Result<Option<u32>> {
        let w = wide(name, "process name")?;
        // `ProcessExists` is misnamed: it returns the process id, not a
        // boolean, and 0 for "no such process".
        let (pid, _) = au3!(self, AU3_ProcessExists(w.as_ptr()));
        Ok(if pid == 0 { None } else { Some(pid as u32) })
    }

    pub(crate) fn pixel_get_color(&self, p: Point) -> Result<u32> {
        let (rgb, _) = au3!(self, AU3_PixelGetColor(p.x, p.y));
        // No failure signal: an off-screen coordinate yields 0xFFFFFF, which is
        // also a perfectly ordinary white pixel. Callers that care must bound
        // their coordinates themselves.
        Ok(rgb as u32)
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

    pub(crate) fn mouse_get_pos(&self) -> Result<Point> {
        let mut p = POINT::default();
        au3!(self, AU3_MouseGetPos(&raw mut p));
        Ok(Point::new(p.x, p.y))
    }

    pub(crate) fn mouse_move(&self, p: Point, speed: Option<Speed>) -> Result<()> {
        let spd = speed.map_or(AU3_INTDEFAULT, Speed::get);
        au3!(self, AU3_MouseMove(p.x, p.y, spd));
        Ok(())
    }

    pub(crate) fn mouse_down(&self, button: &str) -> Result<()> {
        let b = wide(button, "mouse button")?;
        au3!(self, AU3_MouseDown(b.as_ptr()));
        Ok(())
    }

    pub(crate) fn mouse_up(&self, button: &str) -> Result<()> {
        let b = wide(button, "mouse button")?;
        au3!(self, AU3_MouseUp(b.as_ptr()));
        Ok(())
    }

    pub(crate) fn mouse_wheel(&self, direction: &str, clicks: u32) -> Result<()> {
        let d = wide(direction, "wheel direction")?;
        au3!(self, AU3_MouseWheel(d.as_ptr(), clicks as i32));
        Ok(())
    }

    pub(crate) fn mouse_click_drag(
        &self,
        button: &str,
        from: Point,
        to: Point,
        speed: Option<Speed>,
    ) -> Result<()> {
        let b = wide(button, "mouse button")?;
        let spd = speed.map_or(AU3_INTDEFAULT, Speed::get);
        let (ok, code) = au3!(
            self,
            AU3_MouseClickDrag(b.as_ptr(), from.x, from.y, to.x, to.y, spd)
        );
        if ok == 0 {
            return Err(Error::AutoItFailed {
                func: "AU3_MouseClickDrag",
                code,
            });
        }
        Ok(())
    }

    pub(crate) fn win_move(&self, s: &Selector, r: Rect) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinMove(w.as_ptr(), EMPTY_WIDE.as_ptr(), r.x, r.y, r.w, r.h)
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_set_title(&self, s: &Selector, title: &str) -> Result<bool> {
        let w = self.sel(s)?;
        let t = wide(title, "new title")?;
        let (ok, _) = au3!(
            self,
            AU3_WinSetTitle(w.as_ptr(), EMPTY_WIDE.as_ptr(), t.as_ptr())
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_set_on_top(&self, s: &Selector, on_top: bool) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinSetOnTop(w.as_ptr(), EMPTY_WIDE.as_ptr(), i32::from(on_top))
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_kill(&self, s: &Selector) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(self, AU3_WinKill(w.as_ptr(), EMPTY_WIDE.as_ptr()));
        Ok(ok != 0)
    }

    pub(crate) fn win_wait_not_active(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let w = self.sel(s)?;
        let (ok, _) = au3!(
            self,
            AU3_WinWaitNotActive(w.as_ptr(), EMPTY_WIDE.as_ptr(), timeout_secs(t))
        );
        Ok(ok != 0)
    }

    pub(crate) fn win_get_client_size(&self, s: &Selector) -> Result<Size> {
        let w = self.sel(s)?;
        let mut rect = RECT::default();
        // Same shape as WinGetPos, so the same rule: the error flag decides.
        let (_, code) = au3!(
            self,
            AU3_WinGetClientSize(w.as_ptr(), EMPTY_WIDE.as_ptr(), &raw mut rect)
        );
        if code != 0 {
            return Err(Error::window_not_found(s));
        }
        let r = Rect::from(rect);
        Ok(Size::new(r.w, r.h))
    }

    pub(crate) fn win_minimize_all(&self, undo: bool) -> Result<()> {
        if undo {
            au3!(self, AU3_WinMinimizeAllUndo());
        } else {
            au3!(self, AU3_WinMinimizeAll());
        }
        Ok(())
    }

    pub(crate) fn process_wait(&self, name: &str, t: Option<Duration>) -> Result<bool> {
        let w = wide(name, "process name")?;
        let (ok, _) = au3!(self, AU3_ProcessWait(w.as_ptr(), timeout_secs(t)));
        Ok(ok != 0)
    }

    pub(crate) fn process_wait_close(&self, name: &str, t: Option<Duration>) -> Result<bool> {
        let w = wide(name, "process name")?;
        let (ok, _) = au3!(self, AU3_ProcessWaitClose(w.as_ptr(), timeout_secs(t)));
        Ok(ok != 0)
    }

    pub(crate) fn run_wait(&self, command: &str, working_dir: Option<&str>) -> Result<i32> {
        let cmd = wide(command, "command")?;
        let dir = match working_dir {
            Some(d) => wide(d, "working directory")?,
            None => vec![0u16],
        };
        let (exit, _) = au3!(
            self,
            AU3_RunWait(cmd.as_ptr(), dir.as_ptr(), AU3_INTDEFAULT)
        );
        Ok(exit)
    }

    pub(crate) fn is_admin(&self) -> bool {
        let (yes, _) = au3!(self, AU3_IsAdmin());
        yes != 0
    }

    pub(crate) fn tool_tip(&self, text: &str, at: Option<Point>) -> Result<()> {
        let t = wide(text, "tooltip text")?;
        let (x, y) = at.map_or((AU3_INTDEFAULT, AU3_INTDEFAULT), |p| (p.x, p.y));
        au3!(self, AU3_ToolTip(t.as_ptr(), x, y));
        Ok(())
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
