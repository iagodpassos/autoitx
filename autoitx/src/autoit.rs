//! The automation handle.

// One backend or the other, never both. `mock-loader` deliberately wins on
// macOS: it is how the DLL marshalling gets tested without a Windows machine.
#[cfg(any(windows, feature = "mock-loader"))]
use crate::backend::dll::ControlState;
#[cfg(any(windows, feature = "mock-loader"))]
use crate::backend::dll::Inner;
#[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
use crate::backend::macos::inner::Inner;
use crate::error::{Error, Result};
use crate::options::{Options, ShowState, Speed, WinState};
use crate::{Keys, Point, Rect, Selector, Size};
// Controls are addressed by HWND, which only the DLL backend has.
#[cfg(any(windows, feature = "mock-loader"))]
use crate::Control;
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
/// let invoices = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]");
///
/// ai.win_wait_activate(&invoices, Some(Duration::from_secs(30)))?;
/// ai.maximize(&invoices)?;
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
            .field("options", &self.inner.options())
            .finish_non_exhaustive()
    }
}

/// A window state to watch for, for [`AutoIt::wait_for_any`].
///
/// The four map onto AutoIt's four single-window waits — `WinWait`,
/// `WinWaitClose`, `WinWaitActive`, `WinWaitNotActive` — so that racing them
/// against each other needs no vocabulary the caller does not already have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WinCondition {
    /// Some window matches the selector.
    Exists,
    /// No window matches the selector.
    Gone,
    /// The matching window is focused.
    Active,
    /// The matching window is not focused.
    NotActive,
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
    pub fn options(&self) -> Options {
        self.inner.options()
    }

    /// Sets one AutoIt option by name, returning its previous value.
    ///
    /// The escape hatch for options [`Options`] does not model. Names are
    /// AutoIt's own: `"MouseCoordMode"`, `"WinTitleMatchMode"`,
    /// `"SendKeyDelay"`, and the rest.
    ///
    /// ```no_run
    /// # use autoitx::AutoIt;
    /// # let ai = AutoIt::new()?;
    /// // Match window titles as a substring rather than a prefix.
    /// let previous = ai.set_option("WinTitleMatchMode", 2)?;
    /// # Ok::<(), autoitx::Error>(())
    /// ```
    ///
    /// # This is global and sticky
    ///
    /// AutoIt's options are process-wide and outlive the call that set them, so
    /// a change here affects every later call from every thread. That is why
    /// [`Options`] exists: it names the defaults automation silently depends on,
    /// so a change is a visible decision rather than an inherited surprise.
    pub fn set_option(&self, option: &str, value: i32) -> Result<i32> {
        self.inner.set_option(option, value)
    }

    /// Reads one AutoIt option without changing it.
    ///
    /// Uses the sentinel AutoIt provides for exactly this, which is also how
    /// the defaults-parity test checks that [`Options::default`] still matches
    /// what the DLL installs.
    pub fn get_option(&self, option: &str) -> Result<i32> {
        self.inner.set_option(option, autoitx_sys::AU3_INTDEFAULT)
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
    /// Returns `None` where the counter is unavailable — on Windows, in the
    /// rare case `user32` cannot be opened.
    ///
    /// Prefer [`recipes::read_screen_text`](crate::recipes::read_screen_text),
    /// which uses this correctly.
    #[must_use]
    pub fn clip_sequence(&self) -> Option<u32> {
        self.inner.clip_sequence()
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

    /// Where the cursor is.
    pub fn mouse_get_pos(&self) -> Result<Point> {
        self.inner.mouse_get_pos()
    }

    /// Moves the cursor without clicking.
    pub fn mouse_move(&self, p: Point, speed: Option<Speed>) -> Result<()> {
        self.inner.mouse_move(p, speed)
    }

    /// Presses a mouse button and leaves it down.
    ///
    /// Pair with [`mouse_up`](Self::mouse_up). A button left down survives the
    /// end of your program and is confusing to whoever is at the machine, so
    /// prefer [`mouse_click_drag`](Self::mouse_click_drag) where it fits.
    pub fn mouse_down(&self, button: MouseButton) -> Result<()> {
        self.inner.mouse_down(button.as_str())
    }

    /// Releases a mouse button.
    pub fn mouse_up(&self, button: MouseButton) -> Result<()> {
        self.inner.mouse_up(button.as_str())
    }

    /// Scrolls the wheel. `direction` is `"up"` or `"down"`.
    pub fn mouse_wheel(&self, direction: &str, clicks: u32) -> Result<()> {
        self.inner.mouse_wheel(direction, clicks)
    }

    /// Drags from one point to another.
    pub fn mouse_click_drag(
        &self,
        button: MouseButton,
        from: Point,
        to: Point,
        speed: Option<Speed>,
    ) -> Result<()> {
        self.inner
            .mouse_click_drag(button.as_str(), from, to, speed)
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

    /// Waits for the first of several window conditions to hold.
    ///
    /// Returns the index of the watch that fired, or `None` on timeout.
    ///
    /// An action in a legacy application rarely has one outcome. Committing a
    /// form either closes it, or raises an error dialog, or raises a *different*
    /// dialog that means something else entirely — and which one happens is the
    /// only way to find out what the application did. The single-window waits
    /// cannot express that, so automation writes the race by hand:
    ///
    /// ```csharp
    /// while (WinExists("Order Selection")
    ///     && !WinExists("[CLASS:ui60Modal_W32]")
    ///     && !WinExists("Blocked")) { Thread.Sleep(300); }
    /// ```
    ///
    /// That loop is in production twice, in two different robots, and it has no
    /// timeout: if none of the three ever happens, the robot hangs forever. The
    /// version here cannot, because the timeout is a parameter rather than
    /// something you remember to add.
    ///
    /// ```no_run
    /// # use autoitx::{AutoIt, Selector, WinCondition};
    /// # use std::time::Duration;
    /// # let ai = AutoIt::new()?;
    /// let orders  = Selector::from("Order Selection");
    /// let modal   = Selector::from("[CLASS:ui60Modal_W32]");
    /// let blocked = Selector::from("Blocked");
    ///
    /// match ai.wait_for_any(
    ///     &[
    ///         (&orders,  WinCondition::Gone),   // the form closed: it saved
    ///         (&modal,   WinCondition::Exists), // an error came up instead
    ///         (&blocked, WinCondition::Exists), // ... or a block notice did
    ///     ],
    ///     Some(Duration::from_secs(60)),
    /// )? {
    ///     Some(0) => println!("saved"),
    ///     Some(1) => println!("error dialog"),
    ///     Some(2) => println!("blocked"),
    ///     _ => println!("nothing happened in 60s — the application is wedged"),
    /// }
    /// # Ok::<(), autoitx::Error>(())
    /// ```
    ///
    /// # Order is significant
    ///
    /// Watches are evaluated in slice order within each pass, so when two hold
    /// at once the lower index wins. Put the outcome you most need to
    /// distinguish first — the alternative is a return value that depends on
    /// polling luck.
    ///
    /// # Interleaving
    ///
    /// The lock is released between passes, deliberately: holding it across a
    /// minute-long wait would stall every other thread. Another thread can
    /// therefore act between two passes. Take a [`session`](Self::session) if
    /// the window set must not be disturbed while this runs.
    ///
    /// Polling runs at `WinWaitDelay` (250 ms by default), the same option that
    /// paces the other waits. An empty slice returns `Ok(None)` at once rather
    /// than waiting for a condition that cannot arrive.
    pub fn wait_for_any(
        &self,
        watches: &[(&Selector, WinCondition)],
        timeout: Option<Duration>,
    ) -> Result<Option<usize>> {
        if watches.is_empty() {
            return Ok(None);
        }

        let poll = self.options().win_wait_delay;
        let start = Instant::now();
        loop {
            for (index, (selector, condition)) in watches.iter().enumerate() {
                let held = match condition {
                    WinCondition::Exists => self.win_exists(selector)?,
                    WinCondition::Gone => !self.win_exists(selector)?,
                    WinCondition::Active => self.win_active(selector)?,
                    WinCondition::NotActive => !self.win_active(selector)?,
                };
                if held {
                    return Ok(Some(index));
                }
            }
            // Checked after a full pass, so every watch is evaluated at least
            // once even with a zero timeout.
            if timeout.is_some_and(|limit| start.elapsed() >= limit) {
                return Ok(None);
            }
            self.sleep(poll);
        }
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

    /// Resolves a selector to the handle of the window it matches right now.
    ///
    /// Useful for pinning an identity before a sequence of operations: a title
    /// prefix or `[ACTIVE]` names "whatever matches at this instant", and the
    /// answer can change between two calls.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches.
    pub fn win_get_handle(&self, s: &Selector) -> Result<u64> {
        self.inner.win_get_handle(s)
    }

    /// Closes a window if it exists, escalating to killing the process.
    ///
    /// Asks it to close; waits; if it is still there, terminates the owning
    /// process; waits again. Returns `false` if no window matched to begin
    /// with.
    ///
    /// # The selector is pinned first
    ///
    /// Before doing anything, the selector is resolved to a window handle, and
    /// the rest of the sequence targets that handle. This matters more than it
    /// looks:
    ///
    /// `[ACTIVE]` and bare-title selectors mean "whatever matches right now".
    /// Over a close-and-wait sequence that is actively wrong — the application
    /// pops a "save changes?" dialog, which becomes the active window; kill the
    /// process and some *other* application becomes active. A wait for
    /// `[ACTIVE]` to disappear then never finishes, because there is always an
    /// active window somewhere. The close succeeded and the caller is told it
    /// timed out.
    ///
    /// Pinning the handle up front makes the whole sequence refer to the one
    /// window the caller meant.
    ///
    /// # Errors
    ///
    /// [`Error::Timeout`] if the window outlives its own process being
    /// terminated — bounded on purpose, where hand-written automation typically
    /// waits forever.
    pub fn win_close_if_exists(&self, s: &Selector, grace: Duration) -> Result<bool> {
        let _session = self.session();

        // Pin the identity. A failure here means nothing matched, which is the
        // same answer as `win_exists` returning false.
        let Ok(handle) = self.win_get_handle(s) else {
            return Ok(false);
        };
        let target = Selector::handle(handle);

        self.win_close(&target)?;
        if self.win_wait_close(&target, Some(grace))? {
            return Ok(true);
        }

        // It ignored the close request. Ask the OS.
        let pid = self.win_get_process(&target)?;
        if pid != 0 {
            self.inner.process_close(&pid.to_string())?;
        }

        // Bounded, unlike the .NET code's unbounded final wait: a process that
        // survives being terminated is a problem to report, not to hang on.
        if !self.win_wait_close(&target, Some(grace))? {
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

    /// Waits for a process to start. `false` on timeout.
    pub fn process_wait(&self, name: &str, timeout: Option<Duration>) -> Result<bool> {
        self.inner.process_wait(name, timeout)
    }

    /// Waits for a process to end. `false` on timeout.
    pub fn process_wait_close(&self, name: &str, timeout: Option<Duration>) -> Result<bool> {
        self.inner.process_wait_close(name, timeout)
    }

    /// Runs a program and waits for it, returning its exit code.
    pub fn run_wait(&self, command: &str, working_dir: Option<&str>) -> Result<i32> {
        self.inner.run_wait(command, working_dir)
    }

    /// Whether this process is running elevated.
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.inner.is_admin()
    }

    // -- More windows ------------------------------------------------------

    /// Moves and resizes a window. Returns whether it was found.
    pub fn win_move(&self, s: &Selector, r: Rect) -> Result<bool> {
        self.inner.win_move(s, r)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Changes a window's title. Returns whether it was found.
    pub fn win_set_title(&self, s: &Selector, title: &str) -> Result<bool> {
        self.inner.win_set_title(s, title)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Pins a window above the others, or unpins it.
    pub fn win_set_on_top(&self, s: &Selector, on_top: bool) -> Result<bool> {
        self.inner.win_set_on_top(s, on_top)
    }

    /// Forcibly closes a window.
    ///
    /// Unlike [`win_close`](Self::win_close), this does not let the application
    /// object — no "save changes?" prompt, and no chance to save. Prefer
    /// [`win_close_if_exists`](Self::win_close_if_exists), which asks nicely
    /// first and escalates only if it has to.
    pub fn win_kill(&self, s: &Selector) -> Result<bool> {
        self.inner.win_kill(s)
    }

    /// Waits for a window to stop being focused. `false` on timeout.
    pub fn win_wait_not_active(&self, s: &Selector, timeout: Option<Duration>) -> Result<bool> {
        self.inner.win_wait_not_active(s, timeout)
    }

    /// The window's client area, excluding borders and title bar.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] if nothing matches.
    pub fn win_get_client_size(&self, s: &Selector) -> Result<Size> {
        self.inner.win_get_client_size(s)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Minimises every window, as Win+D does.
    pub fn win_minimize_all(&self) -> Result<()> {
        self.inner.win_minimize_all(false)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Undoes [`win_minimize_all`](Self::win_minimize_all).
    pub fn win_minimize_all_undo(&self) -> Result<()> {
        self.inner.win_minimize_all(true)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Shows a tooltip at a screen position, or at the cursor.
    ///
    /// An empty string dismisses it. Handy for showing what a long-running
    /// robot is doing without stealing focus.
    pub fn tool_tip(&self, text: &str, at: Option<Point>) -> Result<()> {
        self.inner.tool_tip(text, at)
    }

    /// Changes a process's scheduling priority.
    ///
    /// `priority` is AutoIt's scale: 0 idle, 1 below normal, 2 normal, 3 above
    /// normal, 4 high, 5 realtime.
    pub fn process_set_priority(&self, name: &str, priority: i32) -> Result<bool> {
        self.inner.process_set_priority(name, priority)
    }

    /// Searches a screen region for a colour.
    ///
    /// `variation` (0–255) lets each channel differ by that much, which is how
    /// you cope with anti-aliasing and subtle theme differences. `step` samples
    /// every nth pixel — faster, at the risk of stepping over a small target.
    ///
    /// `None` means the colour is not there, which is an ordinary answer rather
    /// than a failure: polling for a colour to appear is the usual reason to
    /// call this.
    pub fn pixel_search(
        &self,
        area: Rect,
        colour: u32,
        variation: u32,
        step: u32,
    ) -> Result<Option<Point>> {
        self.inner.pixel_search(area, colour, variation, step)
    }

    /// A checksum of a screen region, for detecting that it changed.
    ///
    /// Cheaper than capturing and comparing images, and enough to answer "has
    /// this part of the screen finished redrawing?". The value is not stable
    /// across machines or themes — compare it only against itself, over time.
    pub fn pixel_checksum(&self, area: Rect, step: u32) -> Result<u32> {
        self.inner.pixel_checksum(area, step)
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

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
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

    // -- Controls ----------------------------------------------------------
    //
    // Addressing a control beats clicking a coordinate: it survives the window
    // moving, the display changing, and the layout shifting. Coordinate clicks
    // are why automation built that way pins a screen resolution and refuses to
    // start when it changes.

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Clicks a control, optionally at a point inside it.
    ///
    /// Returns whether the control was found. `at` defaults to the centre.
    pub fn control_click(
        &self,
        window: &Selector,
        control: &Control,
        button: MouseButton,
        clicks: u32,
        at: Option<Point>,
    ) -> Result<bool> {
        self.inner
            .control_click(window, control, button.as_str(), clicks, at)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Sends keystrokes straight to a control.
    ///
    /// Unlike [`send`](Self::send), this does not require the window to be
    /// focused — which also means it does not steal focus from whoever is at
    /// the machine.
    pub fn control_send(
        &self,
        window: &Selector,
        control: &Control,
        keys: impl AsRef<Keys>,
    ) -> Result<bool> {
        self.inner
            .control_send(window, control, keys.as_ref(), false)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Sends keystrokes literally, with no `{}!+^#` interpretation.
    pub fn control_send_raw(
        &self,
        window: &Selector,
        control: &Control,
        text: &str,
    ) -> Result<bool> {
        self.inner
            .control_send(window, control, &Keys::raw_unchecked(text.to_owned()), true)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Replaces a control's text outright.
    ///
    /// Faster and more reliable than typing into it: no keystrokes, no
    /// auto-complete interfering, no focus required.
    pub fn control_set_text(
        &self,
        window: &Selector,
        control: &Control,
        text: &str,
    ) -> Result<bool> {
        self.inner.control_set_text(window, control, text)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Reads a control's text.
    ///
    /// The direct answer to what automation usually gets by selecting, copying
    /// and reading the clipboard — no keystrokes and no clipboard involved.
    pub fn control_get_text(&self, window: &Selector, control: &Control) -> Result<String> {
        self.inner.control_get_text(window, control)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// The `ClassnameNN` of whichever control has focus.
    ///
    /// Useful for discovering what a window contains while it is on screen.
    pub fn control_get_focus(&self, window: &Selector) -> Result<String> {
        self.inner.control_get_focus(window)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// A control's position and size, relative to its window.
    ///
    /// # Errors
    ///
    /// [`Error::ControlNotFound`] if the window has no such control.
    pub fn control_get_pos(&self, window: &Selector, control: &Control) -> Result<Rect> {
        self.inner.control_get_pos(window, control)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// A control's window handle.
    ///
    /// # Errors
    ///
    /// [`Error::WindowNotFound`] or [`Error::ControlNotFound`].
    pub fn control_get_handle(&self, window: &Selector, control: &Control) -> Result<u64> {
        self.inner.control_get_handle(window, control)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Moves and resizes a control within its window.
    pub fn control_move(&self, window: &Selector, control: &Control, r: Rect) -> Result<bool> {
        self.inner.control_move(window, control, r)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Gives a control keyboard focus.
    pub fn control_focus(&self, window: &Selector, control: &Control) -> Result<bool> {
        self.inner
            .control_set_state(window, control, ControlState::Focus)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Enables a control for input.
    pub fn control_enable(&self, window: &Selector, control: &Control) -> Result<bool> {
        self.inner
            .control_set_state(window, control, ControlState::Enable)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Greys a control out.
    pub fn control_disable(&self, window: &Selector, control: &Control) -> Result<bool> {
        self.inner
            .control_set_state(window, control, ControlState::Disable)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Shows a hidden control.
    pub fn control_show(&self, window: &Selector, control: &Control) -> Result<bool> {
        self.inner
            .control_set_state(window, control, ControlState::Show)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Hides a control.
    pub fn control_hide(&self, window: &Selector, control: &Control) -> Result<bool> {
        self.inner
            .control_set_state(window, control, ControlState::Hide)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Runs one of AutoIt's control commands.
    ///
    /// The catch-all for widget-specific operations — `"IsChecked"`,
    /// `"Check"`, `"ShowDropDown"`, `"SelectString"`, `"GetCurrentLine"` and
    /// the rest. See AutoIt's `ControlCommand` documentation for the full set;
    /// the command and its argument are passed through untouched.
    pub fn control_command(
        &self,
        window: &Selector,
        control: &Control,
        command: &str,
        extra: &str,
    ) -> Result<String> {
        self.inner.control_command(window, control, command, extra)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Runs one of AutoIt's list-view commands, e.g. `"GetItemCount"`.
    pub fn control_list_view(
        &self,
        window: &Selector,
        control: &Control,
        command: &str,
        extra1: &str,
        extra2: &str,
    ) -> Result<String> {
        self.inner
            .control_list_view(window, control, command, extra1, extra2)
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    /// Runs one of AutoIt's tree-view commands, e.g. `"Expand"`.
    pub fn control_tree_view(
        &self,
        window: &Selector,
        control: &Control,
        command: &str,
        extra1: &str,
        extra2: &str,
    ) -> Result<String> {
        self.inner
            .control_tree_view(window, control, command, extra1, extra2)
    }

    /// The shape of the system mouse cursor.
    ///
    /// Windows-only: macOS has no public API for the system-wide cursor shape.
    /// Prefer [`recipes::wait_until_idle`](crate::recipes::wait_until_idle),
    /// which expresses the intent portably.
    #[cfg(any(windows, docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    pub fn mouse_get_cursor(&self) -> Result<crate::ext::windows::MouseCursor> {
        Ok(crate::ext::windows::MouseCursor::from_code(
            self.inner.mouse_get_cursor()?,
        ))
    }

    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
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
        #[cfg(any(windows, feature = "mock-loader"))]
        let inner = Inner::load(self.dll_path.as_deref(), self.options, self.max_chars)?;

        // The macOS backend has no library to find and no strings to marshal,
        // so `dll_path` and `max_string_chars` have nothing to act on. They stay
        // on the builder rather than becoming compile errors: a flow configured
        // once should build for both targets.
        #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
        let inner = Inner::load(self.options)?;

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
