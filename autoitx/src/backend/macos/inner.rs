//! The macOS side of [`AutoIt`](crate::AutoIt).
//!
//! Same method surface as [`dll::Inner`](crate::backend::dll), over Apple's
//! frameworks instead of AutoItX. `AutoIt` picks between the two by `cfg` and
//! is otherwise unaware of which one it has.
//!
//! # Window handles
//!
//! Win32 hands out an `HWND` that names one window for as long as it exists.
//! macOS has no public equivalent — `AXUIElement` is a live object rather than
//! a token, and the `CGWindowID` that would serve is only reachable through a
//! private function.
//!
//! So handles are minted here: a hash of the owning process and the window
//! title, remembered in a small table. That gives the property the API
//! actually depends on — a handle keeps meaning the same window while a
//! sequence of operations runs, so
//! [`win_close_if_exists`](crate::AutoIt::win_close_if_exists) can pin what it
//! is closing and not follow the focus onto a "save changes?" dialog.
//!
//! What it does not give is uniqueness across two windows of one application
//! with identical titles. Win32 handles do; these do not, and no public macOS
//! API closes that gap.

use super::{clipboard, input, permissions, pixel, process, window};
use crate::error::{Error, Result};
use crate::keys::Keys;
use crate::options::{Options, ShowState, Speed, WinState};
use crate::selector::Criterion;
use crate::{Point, Rect, Selector, Size};
use parking_lot::{Mutex, ReentrantMutex, ReentrantMutexGuard, RwLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Everything shared between clones of a handle to the automation session.
pub(crate) struct Inner {
    /// Serialises access, so two flows cannot interleave their keystrokes.
    /// Reentrant so a [`Session`](crate::Session) can hold it across many calls
    /// without the per-call locks deadlocking.
    lock: ReentrantMutex<()>,
    /// Mutable because [`set_option`](Self::set_option) exists; AutoIt's
    /// options are process-wide and settable at any time, and the macOS backend
    /// emulates that rather than freezing them at build time.
    options: RwLock<Options>,
    /// Minted window handles. See the module docs for what they guarantee.
    handles: Mutex<HashMap<u64, Pinned>>,
}

/// What a minted handle remembers.
#[derive(Clone)]
struct Pinned {
    pid: i32,
    title: String,
}

impl Inner {
    /// Builds the backend.
    ///
    /// # Errors
    ///
    /// [`Error::PermissionDenied`] if Accessibility has not been granted.
    /// Checked here rather than at the first call because without it every AX
    /// query returns "not found", and an automation that fails at step 40 with
    /// "window not found" sends you hunting for a selector bug that is not
    /// there.
    pub(crate) fn load(options: Options) -> Result<Self> {
        permissions::require(permissions::Permission::Accessibility)?;
        Ok(Self {
            lock: ReentrantMutex::new(()),
            options: RwLock::new(options),
            handles: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn lock(&self) -> ReentrantMutexGuard<'_, ()> {
        self.lock.lock()
    }

    pub(crate) fn options(&self) -> Options {
        *self.options.read()
    }

    // -- Resolving selectors -----------------------------------------------

    /// Finds the window a selector names.
    fn resolve(&self, s: &Selector) -> Result<window::Window> {
        self.find(s)?.ok_or_else(|| Error::window_not_found(s))
    }

    /// Finds the window a selector names, if it is there.
    fn find(&self, s: &Selector) -> Result<Option<window::Window>> {
        if let Some(pinned) = self.pinned(s) {
            return self.find_pinned(&pinned);
        }
        let o = self.options();
        window::find(
            s,
            o.win_title_match_mode,
            o.win_title_match_case_insensitive,
        )
    }

    /// The pin behind a handle selector, if this selector is one.
    fn pinned(&self, s: &Selector) -> Option<Pinned> {
        let criteria = s.criteria()?;
        let handle = criteria.iter().find_map(|c| match c {
            Criterion::Handle(h) => Some(*h),
            _ => None,
        })?;
        self.handles.lock().get(&handle).cloned()
    }

    /// Re-finds a pinned window.
    ///
    /// Exact title match on the same process — deliberately stricter than the
    /// prefix matching a bare title gets, because the whole point of pinning is
    /// that a dialog appearing over the window must not answer to the handle.
    fn find_pinned(&self, pinned: &Pinned) -> Result<Option<window::Window>> {
        Ok(window::all_windows()?
            .into_iter()
            .find(|w| w.pid == pinned.pid && w.title == pinned.title))
    }

    /// Mints, or recalls, the handle for a window.
    fn handle_for(&self, w: &window::Window) -> u64 {
        // Deterministic, so asking twice about one window gives one answer —
        // which is how HWNDs behave and what callers compare against.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in (w.pid as u32)
            .to_le_bytes()
            .iter()
            .chain(w.title.as_bytes())
        {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Zero means "no window" throughout the API, so never mint it.
        let handle = if hash == 0 { 1 } else { hash };

        self.handles.lock().insert(
            handle,
            Pinned {
                pid: w.pid,
                title: w.title.clone(),
            },
        );
        handle
    }

    /// Polls until `f` reports what the caller is waiting for, or time runs out.
    ///
    /// `None` means wait forever, matching AutoIt's reading of a zero timeout.
    fn wait_for(
        &self,
        timeout: Option<Duration>,
        mut f: impl FnMut() -> Result<bool>,
    ) -> Result<bool> {
        let delay = self.options().win_wait_delay;
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if f()? {
                return Ok(true);
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Ok(false);
            }
            std::thread::sleep(delay);
        }
    }

    // -- Keyboard ----------------------------------------------------------

    pub(crate) fn send(&self, keys: &Keys) -> Result<()> {
        let _guard = self.lock();
        input::send(keys, &self.options())
    }

    // -- Clipboard ---------------------------------------------------------

    pub(crate) fn clip_get(&self) -> Result<String> {
        let _guard = self.lock();
        clipboard::get()
    }

    /// The pasteboard's change counter.
    pub(crate) fn clip_sequence(&self) -> Option<u32> {
        clipboard::sequence()
    }

    pub(crate) fn clip_put(&self, s: &str) -> Result<()> {
        let _guard = self.lock();
        clipboard::put(s)
    }

    /// Whether the target application is ready for input.
    ///
    /// A direct measurement rather than the inference Windows is stuck with:
    /// this asks the frontmost application a question and sees whether it
    /// answers inside the timeout. A beachballing app fails the round trip.
    ///
    /// The frontmost application is the subject because that is what the
    /// cursor shape reports on Windows — both answer "is the thing I am about
    /// to type into ready for me".
    pub(crate) fn is_idle(&self) -> Result<bool> {
        let Some(app) = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication() else {
            // Nothing is frontmost — a locked screen, or a login window. There
            // is nothing to be ready, so this is not "idle".
            return Ok(false);
        };
        Ok(window::is_app_responsive(
            app.processIdentifier(),
            Duration::from_millis(250),
        ))
    }

    // -- Mouse -------------------------------------------------------------

    pub(crate) fn mouse_click(
        &self,
        button: &str,
        at: Point,
        clicks: u32,
        speed: Option<Speed>,
    ) -> Result<()> {
        let _guard = self.lock();
        input::mouse_click(button, at, clicks, speed, &self.options())
    }

    pub(crate) fn mouse_get_pos(&self) -> Result<Point> {
        let _guard = self.lock();
        input::mouse_get_pos()
    }

    pub(crate) fn mouse_move(&self, p: Point, speed: Option<Speed>) -> Result<()> {
        let _guard = self.lock();
        input::mouse_move(p, speed)
    }

    pub(crate) fn mouse_down(&self, button: &str) -> Result<()> {
        let _guard = self.lock();
        input::mouse_down(button)
    }

    pub(crate) fn mouse_up(&self, button: &str) -> Result<()> {
        let _guard = self.lock();
        input::mouse_up(button)
    }

    pub(crate) fn mouse_wheel(&self, direction: &str, clicks: u32) -> Result<()> {
        let _guard = self.lock();
        input::mouse_wheel(direction, clicks)
    }

    pub(crate) fn mouse_click_drag(
        &self,
        button: &str,
        from: Point,
        to: Point,
        speed: Option<Speed>,
    ) -> Result<()> {
        let _guard = self.lock();
        input::mouse_click_drag(button, from, to, speed, &self.options())
    }

    // -- Windows -----------------------------------------------------------

    pub(crate) fn win_exists(&self, s: &Selector) -> Result<bool> {
        let _guard = self.lock();
        Ok(self.find(s)?.is_some())
    }

    pub(crate) fn win_active(&self, s: &Selector) -> Result<bool> {
        let _guard = self.lock();
        Ok(self.find(s)?.is_some_and(|w| window::is_active(&w)))
    }

    pub(crate) fn win_activate(&self, s: &Selector) -> Result<bool> {
        let _guard = self.lock();
        let Some(w) = self.find(s)? else {
            return Ok(false);
        };
        Ok(window::activate(&w))
    }

    pub(crate) fn win_wait(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let _guard = self.lock();
        self.wait_for(t, || Ok(self.find(s)?.is_some()))
    }

    pub(crate) fn win_wait_active(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let _guard = self.lock();
        self.wait_for(t, || {
            Ok(self.find(s)?.is_some_and(|w| window::is_active(&w)))
        })
    }

    pub(crate) fn win_wait_not_active(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let _guard = self.lock();
        self.wait_for(t, || {
            Ok(!self.find(s)?.is_some_and(|w| window::is_active(&w)))
        })
    }

    pub(crate) fn win_wait_close(&self, s: &Selector, t: Option<Duration>) -> Result<bool> {
        let _guard = self.lock();
        self.wait_for(t, || Ok(self.find(s)?.is_none()))
    }

    pub(crate) fn win_close(&self, s: &Selector) -> Result<()> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        window::close(&w);
        Ok(())
    }

    pub(crate) fn win_kill(&self, s: &Selector) -> Result<bool> {
        let _guard = self.lock();
        let Some(w) = self.find(s)? else {
            return Ok(false);
        };
        // No macOS equivalent of destroying one window by force, so this is the
        // owning application. Same escalation Windows' WinKill performs when a
        // window ignores being closed, one level up.
        Ok(process::close(w.pid))
    }

    pub(crate) fn win_get_process(&self, s: &Selector) -> Result<u32> {
        let _guard = self.lock();
        Ok(self.resolve(s)?.pid as u32)
    }

    pub(crate) fn win_get_handle(&self, s: &Selector) -> Result<u64> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        Ok(self.handle_for(&w))
    }

    pub(crate) fn win_get_title(&self, s: &Selector) -> Result<String> {
        let _guard = self.lock();
        Ok(self.resolve(s)?.title)
    }

    pub(crate) fn win_get_pos(&self, s: &Selector) -> Result<Rect> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        w.rect().ok_or(Error::Platform {
            operation: "read the window's position",
            platform: "macOS",
        })
    }

    pub(crate) fn win_get_client_size(&self, s: &Selector) -> Result<Size> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        // AX reports the window frame. macOS has no separate "client area" —
        // the title bar is drawn inside the frame and its height is up to the
        // application, so there is nothing honest to subtract.
        w.size().ok_or(Error::Platform {
            operation: "read the window's size",
            platform: "macOS",
        })
    }

    pub(crate) fn win_get_state(&self, s: &Selector) -> Result<WinState> {
        let _guard = self.lock();
        match self.find(s)? {
            Some(w) => Ok(window::state(&w)),
            // Not an error: "does this window exist" is exactly what the EXISTS
            // bit answers, and AutoIt reports an empty state the same way.
            None => Ok(WinState::empty()),
        }
    }

    pub(crate) fn win_set_state(&self, s: &Selector, state: ShowState) -> Result<bool> {
        let _guard = self.lock();
        let Some(w) = self.find(s)? else {
            return Ok(false);
        };
        Ok(window::set_show_state(&w, state))
    }

    pub(crate) fn win_move(&self, s: &Selector, r: Rect) -> Result<bool> {
        let _guard = self.lock();
        let Some(w) = self.find(s)? else {
            return Ok(false);
        };
        Ok(window::set_rect(&w, r))
    }

    pub(crate) fn win_get_text(&self, s: &Selector) -> Result<String> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        Ok(window::text_of(&w))
    }

    pub(crate) fn win_get_class_list(&self, s: &Selector) -> Result<Vec<String>> {
        let _guard = self.lock();
        let w = self.resolve(s)?;
        Ok(window::roles_of(&w))
    }

    // -- Processes ---------------------------------------------------------

    pub(crate) fn process_id(&self, name: &str) -> Result<Option<u32>> {
        Ok(process::find(name).map(|pid| pid as u32))
    }

    pub(crate) fn process_close(&self, name_or_pid: &str) -> Result<()> {
        if let Some(pid) = process::find(name_or_pid) {
            process::close(pid);
        }
        // A process that is already gone is the outcome that was asked for.
        Ok(())
    }

    pub(crate) fn process_wait(&self, name: &str, t: Option<Duration>) -> Result<bool> {
        self.wait_for(t, || Ok(process::find(name).is_some()))
    }

    pub(crate) fn process_wait_close(&self, name: &str, t: Option<Duration>) -> Result<bool> {
        self.wait_for(t, || Ok(process::find(name).is_none()))
    }

    pub(crate) fn process_set_priority(&self, name: &str, priority: i32) -> Result<bool> {
        Ok(process::find(name).is_some_and(|pid| process::set_priority(pid, priority)))
    }

    pub(crate) fn run(&self, command: &str, working_dir: Option<&str>) -> Result<u32> {
        Ok(spawn(command, working_dir)?.id())
    }

    pub(crate) fn run_wait(&self, command: &str, working_dir: Option<&str>) -> Result<i32> {
        let status = spawn(command, working_dir)?.wait()?;
        // A process killed by a signal has no exit code. Reporting the signal
        // as a negative number is the shell convention and keeps "it did not
        // exit cleanly" distinguishable from "it exited with 0".
        Ok(status.code().unwrap_or(-1))
    }

    pub(crate) fn is_admin(&self) -> bool {
        process::is_root()
    }

    // -- Pixels ------------------------------------------------------------

    pub(crate) fn pixel_get_color(&self, p: Point) -> Result<u32> {
        let _guard = self.lock();
        pixel::color_at(p)
    }

    pub(crate) fn pixel_search(
        &self,
        area: Rect,
        colour: u32,
        variation: u32,
        step: u32,
    ) -> Result<Option<Point>> {
        let _guard = self.lock();
        Ok(pixel::Capture::of(area)?.search(colour, variation, step))
    }

    pub(crate) fn pixel_checksum(&self, area: Rect, step: u32) -> Result<u32> {
        let _guard = self.lock();
        Ok(pixel::Capture::of(area)?.checksum(step))
    }

    // -- Options and timing ------------------------------------------------

    /// Sets one option by AutoIt's name, returning its previous value.
    ///
    /// The sentinel AutoIt uses to read an option without changing it is
    /// honoured, because that is how the defaults-parity test works and how
    /// [`get_option`](crate::AutoIt::get_option) is built.
    pub(crate) fn set_option(&self, option: &str, value: i32) -> Result<i32> {
        let mut options = self.options.write();
        let read_only = value == autoitx_sys::AU3_INTDEFAULT;
        crate::options::apply_named(&mut options, option, value, read_only)
    }

    pub(crate) fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Runs a command line the way `Run` does: through the shell.
///
/// AutoIt takes a command *line*, not a program and arguments, and automation
/// ported from it passes strings with quoting and redirection already in them.
/// Splitting on spaces here would break every one of those, so the shell does
/// the parsing it was written for.
fn spawn(command: &str, working_dir: Option<&str>) -> std::io::Result<std::process::Child> {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(command);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.spawn()
}

// SAFETY: `Inner` owns only a lock, an `RwLock<Options>` and a `Mutex<HashMap>`
// of plain data. No `AXUIElement` or other Core Foundation object is stored —
// they are created and dropped inside each call — so there is nothing here with
// thread affinity.
unsafe impl Send for Inner {}
// SAFETY: as above.
unsafe impl Sync for Inner {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::TitleMatchMode;

    /// The backend without the permission check, for testing the parts that do
    /// not touch the Accessibility API.
    fn offline() -> Inner {
        Inner {
            lock: ReentrantMutex::new(()),
            options: RwLock::new(Options::default()),
            handles: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn a_handle_is_stable_for_the_same_window() {
        // What `win_close_if_exists` relies on: pinning a window and asking
        // again has to give the same token, or the pin means nothing.
        let inner = offline();
        let w = window::Window::for_test(501, "Untitled - TextEdit");
        let first = inner.handle_for(&w);
        let second = inner.handle_for(&w);
        assert_eq!(first, second);
        assert_ne!(first, 0);
    }

    #[test]
    fn different_windows_get_different_handles() {
        let inner = offline();
        let a = inner.handle_for(&window::Window::for_test(501, "Report"));
        let b = inner.handle_for(&window::Window::for_test(501, "Invoice"));
        let c = inner.handle_for(&window::Window::for_test(502, "Report"));
        assert_ne!(a, b, "same process, different titles");
        assert_ne!(a, c, "same title, different processes");
    }

    #[test]
    fn a_handle_selector_resolves_back_to_what_was_pinned() {
        let inner = offline();
        let handle = inner.handle_for(&window::Window::for_test(4242, "Save changes?"));

        let pinned = inner
            .pinned(&Selector::handle(handle))
            .expect("the handle was just minted");
        assert_eq!(pinned.pid, 4242);
        assert_eq!(pinned.title, "Save changes?");
    }

    #[test]
    fn an_unknown_handle_is_not_mistaken_for_an_unpinned_selector() {
        // If this returned `None`, the selector would fall through to ordinary
        // matching and `[HANDLE:...]` would quietly match the wrong window.
        let inner = offline();
        assert!(inner.pinned(&Selector::handle(0xDEAD_BEEF)).is_none());
    }

    #[test]
    fn options_can_be_read_and_set_by_autoit_name() {
        let inner = offline();
        assert_eq!(
            inner.options().win_title_match_mode,
            TitleMatchMode::StartsWith
        );

        let previous = inner
            .set_option("WinTitleMatchMode", 2)
            .expect("known option");
        assert_eq!(previous, 1);
        assert_eq!(
            inner.options().win_title_match_mode,
            TitleMatchMode::Substring
        );
    }

    #[test]
    fn the_read_only_sentinel_reports_without_changing() {
        // The mechanism behind `get_option`, and behind the parity test that
        // checks `Options::default` still matches what AutoIt installs.
        let inner = offline();
        let value = inner
            .set_option("SendKeyDelay", autoitx_sys::AU3_INTDEFAULT)
            .expect("known option");
        assert_eq!(value, 5);
        assert_eq!(inner.options().send_key_delay, Duration::from_millis(5));
    }
}
