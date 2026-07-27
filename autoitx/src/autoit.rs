//! The automation handle.

use crate::backend::dll::Inner;
use crate::error::{Error, Result};
use crate::options::{Options, ShowState, Speed, WinState};
use crate::{Keys, Point, Rect, Selector};
use parking_lot::ReentrantMutexGuard;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A handle to an automation session.
///
/// Cheap to clone; every clone shares one loaded DLL and one lock. `Send` and
/// `Sync`, so a supervisor thread can call [`win_exists`](Self::win_exists)
/// while a worker drives a flow.
///
/// ```no_run
/// use autoitx::{AutoIt, Keys, Selector, keys};
/// use std::time::Duration;
///
/// let ai = AutoIt::new()?;
/// let nfe = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]");
///
/// ai.win_wait_activate(&nfe, Some(Duration::from_secs(30)))?;
/// ai.maximize(&nfe)?;
/// ai.send(keys!("{CTRLDOWN}{SHIFTDOWN}j{SHIFTUP}{CTRLUP}"))?;
/// # Ok::<(), autoitx::Error>(())
/// ```
///
/// # Interleaving
///
/// Every call takes the lock, so individual calls never interleave. That is not
/// enough on its own: two flows alternating activate/send still fight over
/// focus. Use [`session`](Self::session) to hold the lock across a run of
/// calls that must not be interrupted.
#[derive(Clone)]
pub struct AutoIt {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AutoIt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoIt")
            .field("options", self.inner.options())
            .finish_non_exhaustive()
    }
}

impl AutoIt {
    /// Loads the AutoItX3 DLL with default options.
    ///
    /// # Errors
    ///
    /// [`Error::Load`] if the DLL cannot be found or opened. The message lists
    /// every path searched.
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Starts configuring a session.
    #[must_use]
    pub fn builder() -> AutoItBuilder {
        AutoItBuilder::default()
    }

    /// The option table in force.
    #[must_use]
    pub fn options(&self) -> &Options {
        self.inner.options()
    }

    /// Takes exclusive control for a run of calls.
    ///
    /// The returned [`Session`] holds the lock until dropped and forwards the
    /// whole API by [`Deref`], so a flow that must not be interrupted reads
    /// exactly like one that may be:
    ///
    /// ```no_run
    /// # use autoitx::{AutoIt, Selector, keys};
    /// # let ai = AutoIt::new()?;
    /// # let win = Selector::from("[ACTIVE]");
    /// let s = ai.session();
    /// s.win_activate(&win)?;
    /// s.send(keys!("{CTRLDOWN}c{CTRLUP}"))?;
    /// let cell = s.clip_get()?;
    /// drop(s);
    /// # Ok::<(), autoitx::Error>(())
    /// ```
    ///
    /// The lock is reentrant, so the per-call locking inside costs nothing
    /// extra.
    #[must_use]
    pub fn session(&self) -> Session<'_> {
        Session {
            guard: self.inner.lock(),
            autoit: self,
        }
    }

    // -- Keyboard ----------------------------------------------------------

    /// Sends a validated key sequence.
    pub fn send(&self, keys: impl AsRef<Keys>) -> Result<()> {
        self.inner.send(keys.as_ref())
    }

    /// Types text literally, escaping anything `Send` would interpret.
    ///
    /// Use this for data — names, amounts, passwords. Whatever is in the string
    /// arrives as itself.
    pub fn send_text(&self, text: &str) -> Result<()> {
        self.inner.send(&Keys::text(text))
    }

    // -- Clipboard ---------------------------------------------------------

    /// Reads the clipboard as text.
    ///
    /// An empty or non-text clipboard yields `Ok("")`, not an error.
    pub fn clip_get(&self) -> Result<String> {
        self.inner.clip_get()
    }

    /// Replaces the clipboard with text.
    pub fn clip_put(&self, s: &str) -> Result<()> {
        self.inner.clip_put(s)
    }

    /// The system clipboard's sequence number.
    ///
    /// Windows increments this on every clipboard write, by any process, so it
    /// answers "did the clipboard change?" without reading or writing the
    /// contents. Comparing contents cannot do that reliably: a sentinel can
    /// collide with the real value, and a write that restores what was already
    /// there is invisible.
    ///
    /// Returns `None` where the counter is unavailable — off Windows, and in
    /// the rare case `user32` cannot be opened.
    ///
    /// Prefer [`recipes::read_screen_text`](crate::recipes::read_screen_text),
    /// which uses this correctly.
    #[must_use]
    pub fn clip_sequence(&self) -> Option<u32> {
        crate::backend::win32::clipboard_sequence()
    }

    // -- Mouse -------------------------------------------------------------

    /// Left-clicks once at an absolute screen point.
    ///
    /// Absolute coordinates only survive if the screen geometry does. Prefer
    /// [`recipes::click_in_window`](crate::recipes::click_in_window), which
    /// anchors to a window instead.
    pub fn mouse_click(&self, p: Point) -> Result<()> {
        self.inner.mouse_click("left", p, 1, None)
    }

    /// Clicks with full control over button, count, and speed.
    pub fn mouse_click_with(
        &self,
        button: MouseButton,
        p: Point,
        clicks: u32,
        speed: Option<Speed>,
    ) -> Result<()> {
        self.inner.mouse_click(button.as_str(), p, clicks, speed)
    }

    // -- Windows -----------------------------------------------------------

    /// Whether any window matches.
    pub fn win_exists(&self, s: &Selector) -> Result<bool> {
        self.inner.win_exists(s)
    }

    /// Whether the matching window is focused.
    pub fn win_active(&self, s: &Selector) -> Result<bool> {
        self.inner.win_active(s)
    }

    /// Brings the matching window to the foreground.
    ///
    /// Returns whether AutoIt reported success — `false` means the window was
    /// not found, or refused to come forward. This is a return value rather
    /// than an error because activation is routinely followed by a wait, and
    /// the wait is the real verification; failing hard here would turn a
    /// transient refusal into an aborted run. See
    /// [`win_wait_activate`](Self::win_wait_activate).
    pub fn win_activate(&self, s: &Selector) -> Result<bool> {
        self.inner.win_activate(s)
    }

    /// Waits for a window to exist.
    ///
    /// Returns `false` on timeout. `None` waits forever.
    pub fn win_wait(&self, s: &Selector, timeout: Option<Duration>) -> Result<bool> {
        self.inner.win_wait(s, timeout)
    }

    /// Waits for a window to become focused.
    pub fn win_wait_active(&self, s: &Selector, timeout: Option<Duration>) -> Result<bool> {
        self.inner.win_wait_active(s, timeout)
    }

    /// Waits for a window to disappear.
    pub fn win_wait_close(&self, s: &Selector, timeout: Option<Duration>) -> Result<bool> {
        self.inner.win_wait_close(s, timeout)
    }

    /// Activates a window and waits until it is focused.
    ///
    /// Checks first, activates only if needed, then waits — the sequence
    /// virtually all AutoIt automation opens with.
    pub fn win_wait_activate(&self, s: &Selector, timeout: Option<Duration>) -> Result<bool> {
        let _session = self.session();
        if !self.win_active(s)? {
            self.win_activate(s)?;
        }
        self.win_wait_active(s, timeout)
    }

    /// Asks a window to close.
    pub fn win_close(&self, s: &Selector) -> Result<()> {
        self.inner.win_close(s)
    }

    /// The process id owning the matching window.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches. AutoIt signals that with
    /// `(DWORD)-1` rather than 0, so this never returns `4294967295` as if it
    /// were a real process id — which is exactly what a caller would then try
    /// to terminate.
    pub fn win_get_process(&self, s: &Selector) -> Result<u32> {
        self.inner.win_get_process(s)
    }

    /// Shows, hides, minimises, maximises, or restores a window.
    ///
    /// Returns whether the window was found.
    pub fn win_set_state(&self, s: &Selector, state: ShowState) -> Result<bool> {
        self.inner.win_set_state(s, state)
    }

    /// Maximises a window. Returns whether it was found.
    pub fn maximize(&self, s: &Selector) -> Result<bool> {
        self.win_set_state(s, ShowState::Maximize)
    }

    /// Whether the window exists, is visible, enabled, active, minimised or
    /// maximised.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches. This one reports through
    /// AutoIt's error flag rather than its return, because 0 — no flags set —
    /// is a legitimate state.
    pub fn win_get_state(&self, s: &Selector) -> Result<WinState> {
        self.inner.win_get_state(s)
    }

    /// The window's position and size.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches.
    pub fn win_get_pos(&self, s: &Selector) -> Result<Rect> {
        self.inner.win_get_pos(s)
    }

    /// The window's title.
    ///
    /// # An empty string is ambiguous
    ///
    /// AutoItX reports **nothing** here: a window that does not exist and a
    /// window with no title both yield `""` with a clear error flag, and there
    /// is no way to tell them apart. Measured, not assumed — and worth knowing,
    /// because the obvious reading of `Ok("")` is "found it, no title".
    ///
    /// Use [`win_exists`](Self::win_exists) first when the difference matters.
    pub fn win_get_title(&self, s: &Selector) -> Result<String> {
        self.inner.win_get_title(s)
    }

    /// The text AutoIt can read from inside the window.
    ///
    /// Carries the same ambiguity as [`win_get_title`](Self::win_get_title):
    /// an empty result may mean the window is missing.
    ///
    /// Note this returns what the window's *controls* report, not what is drawn
    /// — for a modern application it is often internal scaffolding rather than
    /// anything a person would recognise.
    pub fn win_get_text(&self, s: &Selector) -> Result<String> {
        self.inner.win_get_text(s)
    }

    /// The window class names, one per control.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches. Unlike its neighbours,
    /// this one does set AutoIt's error flag.
    pub fn win_get_class_list(&self, s: &Selector) -> Result<Vec<String>> {
        self.inner.win_get_class_list(s)
    }

    /// Closes a window if it exists, escalating to killing the process.
    ///
    /// The sequence, which mirrors what production AutoIt automation does by
    /// hand: ask it to close; wait; if it is still there, terminate the owning
    /// process; wait again.
    ///
    /// Returns `false` if no window matched in the first place.
    pub fn win_close_if_exists(&self, s: &Selector, grace: Duration) -> Result<bool> {
        let _session = self.session();

        if !self.win_exists(s)? {
            return Ok(false);
        }

        self.win_close(s)?;
        if self.win_wait_close(s, Some(grace))? {
            return Ok(true);
        }

        // It ignored the close request. Ask the OS.
        let pid = self.win_get_process(s)?;
        if pid != 0 {
            self.inner.process_close(&pid.to_string())?;
        }

        // Bounded, unlike the .NET code's unbounded final wait: a process that
        // survives SIGKILL-equivalent is a problem to report, not to hang on.
        if !self.win_wait_close(s, Some(grace))? {
            return Err(Error::Timeout {
                operation: "win_close_if_exists",
                waited: grace * 2,
            });
        }
        Ok(true)
    }

    // -- Processes ---------------------------------------------------------

    /// Launches a program, returning its process id.
    ///
    /// `command` is passed to the shell the way `Run` does, so a `.url` or
    /// `.lnk` shortcut works as well as an executable.
    pub fn run(&self, command: &str, working_dir: Option<&str>) -> Result<u32> {
        self.inner.run(command, working_dir)
    }

    /// Terminates a process by name or pid.
    pub fn process_close(&self, name_or_pid: &str) -> Result<()> {
        self.inner.process_close(name_or_pid)
    }

    /// The id of a running process, by executable name.
    ///
    /// Wraps AutoIt's `ProcessExists`, which is misnamed: it returns the
    /// process id rather than a boolean. `None` means no such process.
    pub fn process_id(&self, name: &str) -> Result<Option<u32>> {
        self.inner.process_id(name)
    }

    /// Whether a process is running, by executable name.
    pub fn process_exists(&self, name: &str) -> Result<bool> {
        Ok(self.process_id(name)?.is_some())
    }

    /// The colour of one screen pixel, as `0xRRGGBB`.
    ///
    /// # No failure signal
    ///
    /// A coordinate outside every display returns `0xFFFFFF` — indistinguishable
    /// from a genuinely white pixel. AutoIt provides nothing better, so bound
    /// your coordinates before calling if that matters.
    pub fn pixel_get_color(&self, p: Point) -> Result<u32> {
        self.inner.pixel_get_color(p)
    }

    // -- Timing ------------------------------------------------------------

    /// Sleeps without holding the automation lock.
    pub fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
    }

    /// Polls `f` until it returns `true` or `timeout` elapses.
    ///
    /// The building block for the [`recipes`](crate::recipes) module, exposed
    /// because real automation always needs one more wait condition than a
    /// library anticipated.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] if the condition never held, plus whatever `f`
    /// returns.
    pub fn wait_until(
        &self,
        operation: &'static str,
        timeout: Duration,
        poll: Duration,
        mut f: impl FnMut() -> Result<bool>,
    ) -> Result<()> {
        let start = Instant::now();
        loop {
            if f()? {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                return Err(Error::Timeout {
                    operation,
                    waited: start.elapsed(),
                });
            }
            self.sleep(poll);
        }
    }

    /// The backend, for [`recipes`](crate::recipes) that need a primitive not
    /// worth exposing on its own.
    pub(crate) fn inner(&self) -> &Inner {
        &self.inner
    }

    /// The raw `AU3_*` function table.
    ///
    /// An escape hatch for the parts of AutoItX this crate has not wrapped, and
    /// for finding out what the DLL actually does — the documentation on
    /// autoitscript.com describes AutoIt's *script* functions, and the DLL
    /// variants do not always agree. `WinGetPos` is the cautionary tale: the
    /// script form returns an array, the DLL form fills a `RECT` and leaves its
    /// integer return unspecified.
    ///
    /// # Locking
    ///
    /// This does **not** take the lock. Hold a [`Session`] for as long as you
    /// use the returned table, or you will race with other threads — AutoItX is
    /// not reentrant, and its error flag belongs to whichever call ran last.
    ///
    /// ```no_run
    /// # use autoitx::AutoIt;
    /// # let ai = AutoIt::new()?;
    /// let s = ai.session();          // hold the lock
    /// let au3 = s.raw();
    /// // SAFETY: the signature matches the declaration, and the session
    /// // guarantees no other thread is calling into AutoItX.
    /// let found = unsafe { (au3.AU3_WinMinimizeAll)() };
    /// # Ok::<(), autoitx::Error>(())
    /// ```
    #[must_use]
    pub fn raw(&self) -> &autoitx_sys::Au3 {
        self.inner.raw()
    }

    /// AutoIt's error flag, as left by the most recent call **on this thread**.
    ///
    /// Only meaningful immediately after a [`raw`](Self::raw) call, under the
    /// same [`Session`]. The wrapped methods read it themselves and it is
    /// overwritten by every call, including theirs.
    #[must_use]
    pub fn raw_error(&self) -> i32 {
        self.inner.raw_error()
    }
}

/// Which mouse button to click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary button.
    #[default]
    Left,
    /// The secondary button.
    Right,
    /// The middle button or wheel.
    Middle,
    /// The primary button, honouring a left-handed swap.
    Primary,
    /// The secondary button, honouring a left-handed swap.
    Secondary,
}

impl MouseButton {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
            Self::Primary => "primary",
            Self::Secondary => "secondary",
        }
    }
}

/// Exclusive access to an automation session.
///
/// Created by [`AutoIt::session`]. Forwards the whole API by [`Deref`], and
/// releases the lock when dropped.
pub struct Session<'a> {
    // Field order matters: `guard` is declared first so it drops last, after
    // any borrow through `autoit` has ended.
    #[expect(dead_code, reason = "held for its Drop; the lock is the point")]
    guard: ReentrantMutexGuard<'a, ()>,
    autoit: &'a AutoIt,
}

impl Deref for Session<'_> {
    type Target = AutoIt;

    fn deref(&self) -> &Self::Target {
        self.autoit
    }
}

impl std::fmt::Debug for Session<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").finish_non_exhaustive()
    }
}

/// Configures an [`AutoIt`] before loading.
#[derive(Debug, Default, Clone)]
pub struct AutoItBuilder {
    dll_path: Option<PathBuf>,
    options: Options,
    max_chars: Option<usize>,
}

impl AutoItBuilder {
    /// Loads the DLL from this exact path, with no fallback.
    ///
    /// An explicit path that does not exist is an error. Quietly loading some
    /// other AutoIt build instead is how unreproducible bugs are made.
    #[must_use]
    pub fn dll_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.dll_path = Some(p.into());
        self
    }

    /// Overrides the option table.
    ///
    /// The default reproduces AutoIt's own, which existing automation depends
    /// on — see [`Options`].
    #[must_use]
    pub const fn options(mut self, o: Options) -> Self {
        self.options = o;
        self
    }

    /// Caps how large a returned string may grow, in UTF-16 code units.
    #[must_use]
    pub const fn max_string_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
    }

    /// Loads the DLL.
    ///
    /// # Errors
    ///
    /// [`Error::Load`] if it cannot be found or opened.
    pub fn build(self) -> Result<AutoIt> {
        let inner = Inner::load(self.dll_path.as_deref(), self.options, self.max_chars)?;
        Ok(AutoIt {
            inner: Arc::new(inner),
        })
    }
}

impl AsRef<Keys> for Keys {
    fn as_ref(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoit_is_send_and_sync() {
        // A supervisor thread polling `win_exists` while a worker drives a flow
        // is the normal shape of an RPA, so this must hold.
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AutoIt>();
    }

    #[test]
    fn mouse_buttons_use_autoits_names() {
        assert_eq!(MouseButton::Left.as_str(), "left");
        assert_eq!(MouseButton::default(), MouseButton::Left);
    }

    #[test]
    fn builder_defaults_to_autoits_option_table() {
        let b = AutoIt::builder();
        assert_eq!(b.options, Options::default());
        assert!(b.dll_path.is_none());
    }
}
