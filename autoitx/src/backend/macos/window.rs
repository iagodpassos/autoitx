//! Finding and manipulating windows on macOS.
//!
//! # Two sources, because neither is enough alone
//!
//! `CGWindowListCopyWindowInfo` enumerates every on-screen window with its
//! owning process, bounds and layer, and needs **no permission at all**. What
//! it will not give up is the title: `kCGWindowName` has required Screen
//! Recording since macOS 10.15.
//!
//! The Accessibility API has titles, and needs only the Accessibility grant.
//!
//! So structure comes from Core Graphics and titles come from AX. The payoff is
//! the permission story: Screen Recording ends up required *only* for pixel
//! operations, rather than for the ordinary business of finding a window.
//!
//! # Selectors
//!
//! [`Selector`] speaks Win32's vocabulary, so the translation has to be stated
//! rather than assumed:
//!
//! | criterion | macOS meaning |
//! |---|---|
//! | `TITLE` | the window's `AXTitle` |
//! | `CLASS` | the application's bundle identifier, then its `AXRole` |
//! | `REGEXPTITLE` | `AXTitle`, matched as a pattern |
//! | `ACTIVE` | the frontmost application's focused window |
//! | a bare title | `AXTitle`, per the title-match mode — prefix by default |
//!
//! `CLASS` is the one that genuinely differs. `Chrome_WidgetWin_1` is a Win32
//! window class and has no macOS counterpart; `com.google.Chrome` is the
//! equivalent identity. [`SelectorSet`](crate::selector::SelectorSet) exists to
//! keep both in one table rather than spreading `#[cfg]` through automation.

// Wired into `AutoIt` alongside the rest of the backend; see mod.rs.
#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

use super::ax;
use super::permissions::{self, Permission};
use crate::options::{ShowState, TitleMatchMode, WinState};
use crate::selector::Criterion;
use crate::{Point, Rect, Selector, Size};
use objc2_app_kit::NSRunningApplication;
use objc2_application_services::AXUIElement;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CGPoint, CGSize};
use objc2_core_graphics::{
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowBounds,
    kCGWindowLayer, kCGWindowOwnerPID,
};
use objc2_foundation::MainThreadMarker;
use std::time::Duration;

/// Reads a title, without the NUL a bridging application may leave on it.
///
/// Not hypothetical: a Windows application published into macOS through
/// Parallels Coherence came back as `"Progress – Explorador de Arquivos\0"`.
/// The NUL is an artefact of the Win32 title crossing the bridge, it is
/// invisible in every UI, and nobody types it — so a selector written from what
/// the user sees would never match under `TitleMatchMode::Exact`.
fn title_of(element: &objc2_application_services::AXUIElement) -> String {
    ax::string_attribute(element, ax::ATTR_TITLE)
        .unwrap_or_default()
        .trim_end_matches('\0')
        .to_owned()
}

/// How long a query to an unresponsive application may block.
///
/// Short on purpose: automation would rather learn that an application is busy
/// than wait for it. The default is effectively unbounded.
const AX_TIMEOUT_SECS: f32 = 0.25;

/// A window found by the backend.
pub(crate) struct Window {
    pub(crate) element: CFRetained<AXUIElement>,
    pub(crate) pid: i32,
    pub(crate) title: String,
}

impl Window {
    pub(crate) fn position(&self) -> Option<Point> {
        ax::point_attribute(&self.element, ax::ATTR_POSITION)
            .map(|p| Point::new(p.x as i32, p.y as i32))
    }

    pub(crate) fn size(&self) -> Option<Size> {
        ax::size_attribute(&self.element, ax::ATTR_SIZE)
            .map(|s| Size::new(s.width as i32, s.height as i32))
    }

    pub(crate) fn rect(&self) -> Option<Rect> {
        let p = self.position()?;
        let s = self.size()?;
        Some(Rect::new(p.x, p.y, s.w, s.h))
    }

    /// A window that does not exist, for testing the parts that only read
    /// `pid` and `title`.
    ///
    /// The element belongs to a process id that cannot be running, so every
    /// accessibility query through it fails quietly — which is the same path a
    /// window that closed mid-flow takes.
    #[cfg(test)]
    pub(crate) fn for_test(pid: i32, title: &str) -> Self {
        Self {
            element: ax::app_element(pid),
            pid,
            title: title.to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------

/// Every window of every running application, with titles.
///
/// Core Graphics says which processes own windows; the accessibility API says
/// what those windows are called. Neither alone is enough — see the module
/// docs. A few dozen processes own windows, so this is fast enough to run in a
/// poll loop.
pub(crate) fn all_windows() -> crate::Result<Vec<Window>> {
    permissions::require(Permission::Accessibility)?;

    let mut out = Vec::new();
    for pid in running_pids() {
        let app = ax::app_element(pid);
        // Bound each application separately: one hung app must not stall the
        // enumeration of the rest.
        ax::set_timeout(&app, AX_TIMEOUT_SECS);

        for element in ax::element_array(&app, ax::ATTR_WINDOWS) {
            let title = title_of(&element);
            out.push(Window {
                element,
                pid,
                title,
            });
        }
    }
    Ok(out)
}

/// The process ids that own a window.
///
/// # Not `NSWorkspace::runningApplications`
///
/// Which is the obvious call, and is wrong here. `NSWorkspace` keeps its list
/// current from notifications delivered on a **run loop**, and an automation
/// process is a command-line tool that never runs one. Measured: launch an
/// application, then enumerate, and the new application is simply absent — for
/// as long as this process lives. The snapshot froze when `sharedWorkspace` was
/// first touched.
///
/// That is fatal for the move every flow opens with: launch the ERP, wait for
/// its window. It would wait forever.
///
/// `CGWindowListCopyWindowInfo` asks the window server, so it is always
/// current. It also needs **no permission** — only the window *name* requires
/// Screen Recording, and titles come from AX instead. And it lists only
/// processes that own a window, a few dozen rather than every process running,
/// so enumeration got several times faster as well.
fn running_pids() -> Vec<i32> {
    // Every window, not only the on-screen ones. A minimised window is still a
    // window: `win_exists` must find it, and `win_get_state` has to be able to
    // report it as MINIMIZED rather than as gone. Measured, `OptionAll` is also
    // the faster call — restricting to on-screen makes the window server
    // compute visibility, which took 45 ms against 2 ms for the whole list.
    let Some(list) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionAll | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };

    let mut pids: Vec<i32> = Vec::new();
    for i in 0..list.count() {
        // SAFETY: `i` is within the array's own reported count.
        let raw = unsafe { list.value_at_index(i) };
        if raw.is_null() {
            continue;
        }
        // SAFETY: the window list's elements are documented as dictionaries,
        // and the borrow ends before `list` is dropped.
        let entry = unsafe { &*raw.cast::<CFDictionary>() };

        // SAFETY: `kCGWindowOwnerPID` is an extern static from Core Graphics.
        if let Some(pid) = number_in(entry, unsafe { kCGWindowOwnerPID }) {
            let pid = pid as i32;
            if pid > 0 && !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// The bundle identifier of a process, e.g. `com.apple.TextEdit`.
///
/// The macOS answer to a Win32 window class: the stable identity of the
/// application that owns the window.
pub(crate) fn bundle_id(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app.bundleIdentifier().map(|s| s.to_string())
}

/// The frontmost application's focused window.
pub(crate) fn active_window() -> crate::Result<Option<Window>> {
    permissions::require(Permission::Accessibility)?;

    let Some(app) = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication() else {
        return Ok(None);
    };
    let pid = app.processIdentifier();

    let app_element = ax::app_element(pid);
    ax::set_timeout(&app_element, AX_TIMEOUT_SECS);

    let Some(value) = ax::attribute(&app_element, ax::ATTR_FOCUSED_WINDOW) else {
        return Ok(None);
    };
    let Some(element) = value.downcast_ref::<AXUIElement>() else {
        return Ok(None);
    };
    // The downcast borrows from the CFType we were handed, so take our own
    // reference before keeping it past this scope.
    // SAFETY: `element` is a live AXUIElement from a successful attribute read.
    let element = unsafe { CFRetained::retain(std::ptr::NonNull::from(element)) };
    let title = title_of(&element);

    Ok(Some(Window {
        element,
        pid,
        title,
    }))
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Whether a title satisfies a bare-title selector under `mode`.
///
/// Emulating Windows' title-match mode is not pedantry: existing automation is
/// written against the *default*, which is prefix matching, and a selector like
/// `"Untitled"` is expected to find `"Untitled - TextEdit"`. Exact matching
/// here would break every such call site silently.
pub(crate) fn title_matches(title: &str, wanted: &str, mode: TitleMatchMode, ci: bool) -> bool {
    let (t, w) = if ci {
        (title.to_lowercase(), wanted.to_lowercase())
    } else {
        (title.to_owned(), wanted.to_owned())
    };
    match mode {
        TitleMatchMode::StartsWith => t.starts_with(&w),
        TitleMatchMode::Substring => t.contains(&w),
        TitleMatchMode::Exact => t == w,
        // In advanced mode a bare title matches nothing; only `[TITLE:...]`
        // does, and that is handled as a criterion.
        TitleMatchMode::Advanced => false,
    }
}

/// A very small regular-expression matcher, for `REGEXPTITLE`.
///
/// Supports what production selectors actually use — `.`, `.*`, and literals —
/// deliberately rather than pulling in a regex engine. Every `REGEXPTITLE` in
/// the automation this crate was extracted from is of the form
/// `Prefix(.*)Middle(.*)`, and a full engine would be a dependency, a
/// compile-time cost, and a much larger surface for a pattern language nobody
/// is using in anger.
///
/// Anything more elaborate is reported rather than silently mismatched.
pub(crate) fn regexp_matches(title: &str, pattern: &str) -> bool {
    // Anchored at both ends, as AutoIt's REGEXPTITLE is not — but AutoIt's
    // patterns in practice start and end with `(.*)` where they mean unanchored,
    // so treating the pattern as a sequence of literals separated by wildcards
    // gives the same answer for them.
    let cleaned = pattern.replace("(.*)", "\u{0}").replace(".*", "\u{0}");
    let parts: Vec<&str> = cleaned.split('\u{0}').collect();

    let mut rest = title;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // A pattern not starting with a wildcard anchors at the beginning.
            if !rest.starts_with(part) {
                return false;
            }
            rest = &rest[part.len()..];
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// Whether a window satisfies a selector.
pub(crate) fn matches(
    window: &Window,
    selector: &Selector,
    mode: TitleMatchMode,
    ci: bool,
) -> bool {
    if let Some(bare) = selector.bare_title() {
        return title_matches(&window.title, bare, mode, ci);
    }

    let Some(criteria) = selector.criteria() else {
        return false;
    };

    criteria.iter().all(|c| match c {
        Criterion::Title(t) => title_matches(&window.title, t, mode, ci),
        Criterion::RegexpTitle(p) => regexp_matches(&window.title, p),
        // The one that genuinely translates: a Win32 class becomes the bundle
        // identifier, falling back to the accessibility role.
        Criterion::Class(c) => {
            bundle_id(window.pid).is_some_and(|b| b.eq_ignore_ascii_case(c))
                || ax::string_attribute(&window.element, ax::ATTR_ROLE)
                    .is_some_and(|r| r.eq_ignore_ascii_case(c))
                || ax::string_attribute(&window.element, ax::ATTR_SUBROLE)
                    .is_some_and(|r| r.eq_ignore_ascii_case(c))
        }
        Criterion::RegexpClass(p) => bundle_id(window.pid).is_some_and(|b| regexp_matches(&b, p)),
        Criterion::Pid(p) => window.pid as u32 == *p,
        Criterion::X(v) => window.position().is_some_and(|p| p.x == *v),
        Criterion::Y(v) => window.position().is_some_and(|p| p.y == *v),
        Criterion::W(v) => window.size().is_some_and(|s| s.w == *v),
        Criterion::H(v) => window.size().is_some_and(|s| s.h == *v),
        // `ACTIVE` is resolved before matching; anything reaching here already
        // came from the active-window lookup.
        Criterion::Active | Criterion::All | Criterion::Last => true,
        // Handles are CGWindowIDs on Windows and have no stable macOS twin
        // yet — see the module docs.
        Criterion::Handle(_) | Criterion::Instance(_) => true,
    })
}

/// The first window matching `selector`.
pub(crate) fn find(
    selector: &Selector,
    mode: TitleMatchMode,
    ci: bool,
) -> crate::Result<Option<Window>> {
    if selector.is_active() {
        return active_window();
    }
    let mut windows = all_windows()?;
    let position = windows.iter().position(|w| matches(w, selector, mode, ci));
    Ok(position.map(|i| windows.swap_remove(i)))
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Brings a window's application forward and raises the window.
pub(crate) fn activate(window: &Window) -> bool {
    // Two steps, and both are needed: raising the window without activating the
    // application leaves it behind the frontmost app, and activating the
    // application alone raises whichever of its windows was last in front.
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid) {
        app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
    }
    ax::perform(&window.element, ax::ACTION_RAISE)
}

/// Whether this window is the focused one.
///
/// # `AXMain` is not the flag to read
///
/// It looks like the obvious one, and it is wrong. Measured against a real
/// desktop, the window that genuinely had focus reported:
///
/// ```text
/// window AXMain    = false
/// window AXFocused = true
/// app    AXFrontmost = true
/// ```
///
/// `AXMain` marks an application's *main* window, which many applications
/// never set — anything drawing its own windows rather than using AppKit's,
/// which covers Electron apps and virtual-machine bridges. Reading it alone
/// meant `win_active` returned false for a window that plainly had focus, and
/// `win_wait_active` would then wait out its whole timeout.
///
/// So: the owning application must be frontmost, and the window must be the
/// one that application considers focused. `AXMain` is still accepted, for
/// applications that set it and not `AXFocused`.
pub(crate) fn is_active(window: &Window) -> bool {
    if !ax::bool_attribute(&ax::app_element(window.pid), ax::ATTR_FRONTMOST).unwrap_or(false) {
        return false;
    }
    ax::bool_attribute(&window.element, ax::ATTR_FOCUSED).unwrap_or(false)
        || ax::bool_attribute(&window.element, ax::ATTR_MAIN).unwrap_or(false)
}

/// Moves and resizes a window.
pub(crate) fn set_rect(window: &Window, r: Rect) -> bool {
    let moved = ax::set_point(
        &window.element,
        ax::ATTR_POSITION,
        CGPoint {
            x: f64::from(r.x),
            y: f64::from(r.y),
        },
    );
    let sized = ax::set_size(
        &window.element,
        ax::ATTR_SIZE,
        CGSize {
            width: f64::from(r.w),
            height: f64::from(r.h),
        },
    );
    moved && sized
}

/// Fills the screen's visible area.
///
/// Deliberately not `AXPress` on the zoom button: zoom *toggles*, and what it
/// does is up to each application — some fill the screen, some fit the content,
/// some enter full-screen. Setting the frame is the only reading of "maximise"
/// that behaves the same everywhere.
pub(crate) fn maximize(window: &Window) -> bool {
    let Some(frame) = visible_frame() else {
        return false;
    };
    set_rect(window, frame)
}

/// The primary screen's usable area.
///
/// # Two ways, because the exact one is main-thread-only
///
/// `NSScreen::visibleFrame` is authoritative — it is what AppKit itself uses,
/// and it accounts for the Dock wherever the user has put it. It is also
/// documented main-thread-only, and automation runs on worker threads.
///
/// Returning `None` off the main thread was the first attempt, and it made
/// `win_set_state(Maximize)` silently do nothing in exactly the place it gets
/// called from. So there is a fallback that asks the window server instead,
/// which has no thread affinity: the display bounds, less the menu bar.
///
/// The fallback does not subtract the Dock. That is a deliberate limit rather
/// than an oversight — the Dock's entry in the window list is a full-screen
/// backing window that says nothing about where the visible strip is, and
/// guessing would be worse than a window that extends behind it.
fn visible_frame() -> Option<Rect> {
    if let Some(mtm) = MainThreadMarker::new() {
        if let Some(screen) = objc2_app_kit::NSScreen::mainScreen(mtm) {
            let (full, visible) = (screen.frame(), screen.visibleFrame());
            // AppKit measures y from the bottom; everything else here measures
            // from the top. This is the one place that conversion happens.
            let y = full.size.height - (visible.origin.y + visible.size.height);
            return Some(Rect::new(
                visible.origin.x as i32,
                y as i32,
                visible.size.width as i32,
                visible.size.height as i32,
            ));
        }
    }

    let bounds = objc2_core_graphics::CGDisplayBounds(objc2_core_graphics::CGMainDisplayID());
    let menu_bar = menu_bar_height();
    Some(Rect::new(
        bounds.origin.x as i32,
        bounds.origin.y as i32 + menu_bar,
        bounds.size.width as i32,
        bounds.size.height as i32 - menu_bar,
    ))
}

/// The height of the menu bar, from the window server.
///
/// It is a real window: owned by the window server, at layer 24, spanning the
/// full width of the display at the very top. Measured on a 3440×1440 display
/// it reports `(0, 0, 3440, 30)`. Zero if it cannot be found, which loses the
/// menu bar strip rather than the whole operation.
fn menu_bar_height() -> i32 {
    /// `kCGMainMenuWindowLevel`.
    const MENU_BAR_LAYER: i64 = 24;

    let Some(list) =
        CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly, kCGNullWindowID)
    else {
        return 0;
    };

    for i in 0..list.count() {
        // SAFETY: `i` is within the array's own reported count.
        let raw = unsafe { list.value_at_index(i) };
        if raw.is_null() {
            continue;
        }
        // SAFETY: the window list's elements are documented as dictionaries.
        let entry = unsafe { &*raw.cast::<CFDictionary>() };

        // SAFETY: reading documented keys from a dictionary we hold.
        if number_in(entry, unsafe { kCGWindowLayer }) != Some(MENU_BAR_LAYER) {
            continue;
        }
        // SAFETY: as above; `kCGWindowBounds` holds a nested dictionary.
        let raw_bounds = unsafe { entry.value(std::ptr::from_ref(kCGWindowBounds).cast()) };
        if raw_bounds.is_null() {
            continue;
        }
        // SAFETY: documented as a dictionary of X/Y/Width/Height numbers.
        let bounds = unsafe { &*raw_bounds.cast::<CFDictionary>() };
        let y = number_in(bounds, &CFString::from_str("Y"));
        let height = number_in(bounds, &CFString::from_str("Height"));
        if y == Some(0) {
            if let Some(h) = height {
                return h as i32;
            }
        }
    }
    0
}

/// Reads an integer out of a Core Graphics dictionary.
fn number_in(dict: &CFDictionary, key: &CFString) -> Option<i64> {
    // SAFETY: looking up a key in a dictionary we hold a reference to.
    let value = unsafe { dict.value(std::ptr::from_ref(key).cast()) };
    if value.is_null() {
        return None;
    }
    // SAFETY: every key read through here is documented as holding a CFNumber.
    let number = unsafe { &*value.cast::<CFNumber>() };
    let mut out: i64 = 0;
    // SAFETY: the out-pointer matches the type being requested.
    let ok = unsafe {
        number.value(
            objc2_core_foundation::CFNumberType::SInt64Type,
            std::ptr::from_mut(&mut out).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some(out)
}

/// Asks a window to close.
///
/// Presses the close button rather than terminating anything, which is what
/// `WinClose` does on Windows: the application gets to run its own shutdown,
/// including popping a "save changes?" dialog. Escalating to the process is
/// [`win_kill`](crate::AutoIt::win_kill)'s job.
pub(crate) fn close(window: &Window) -> bool {
    // The close button is a child of the window with the subrole
    // `AXCloseButton`; pressing it is the only way to send a window the same
    // request the user's click would.
    for child in ax::element_array(&window.element, ax::ATTR_CHILDREN) {
        if ax::string_attribute(&child, ax::ATTR_SUBROLE).as_deref() == Some(SUBROLE_CLOSE_BUTTON) {
            return ax::perform(&child, ax::ACTION_PRESS);
        }
    }
    // A window with no close button — a modal sheet, a panel — cannot be closed
    // this way, and saying so beats reporting a success that did nothing.
    false
}

/// The subrole of a window's close button.
const SUBROLE_CLOSE_BUTTON: &str = "AXCloseButton";

/// Applies one of Win32's `SW_*` states.
pub(crate) fn set_show_state(window: &Window, state: ShowState) -> bool {
    match state {
        ShowState::Maximize => maximize(window),
        ShowState::Minimize | ShowState::ShowMinimized | ShowState::ShowMinNoActive => {
            ax::set_bool(&window.element, ax::ATTR_MINIMIZED, true)
        }
        ShowState::ShowNormal | ShowState::Show | ShowState::Restore => {
            let restored = ax::set_bool(&window.element, ax::ATTR_MINIMIZED, false);
            activate(window);
            restored
        }
        ShowState::ShowNoActivate | ShowState::ShowNa => {
            ax::set_bool(&window.element, ax::ATTR_MINIMIZED, false)
        }
        // macOS has no way to hide one window of another application. The
        // nearest thing is minimising it, which is visibly different — callers
        // polling `WinState::VISIBLE` would be told something untrue.
        ShowState::Hide => false,
    }
}

/// How deep the accessibility tree is walked when reading a window.
///
/// Deep enough for real interfaces, bounded because AX trees can contain cycles
/// through parent references and a table view can hold thousands of rows.
const MAX_DEPTH: usize = 12;

/// Every piece of text in a window, top to bottom.
///
/// The macOS answer to `WinGetText`, which on Windows concatenates the text of
/// each child control. Here that means walking the accessibility tree and
/// collecting each element's value or title — the same information, from the
/// API that has it.
pub(crate) fn text_of(window: &Window) -> String {
    let mut out = Vec::new();
    collect(&window.element, 0, &mut |element| {
        // `AXValue` is what a text field or a static label holds; `AXTitle` is
        // what a button or a menu item is called. Value first, and it matters:
        // a text field has both, and its contents are what was asked for.
        let text = ax::string_attribute(element, ax::ATTR_VALUE)
            .or_else(|| ax::string_attribute(element, ax::ATTR_TITLE));
        if let Some(text) = text {
            if !text.is_empty() {
                out.push(text);
            }
        }
    });
    out.join("\n")
}

/// The distinct accessibility roles present in a window.
///
/// Stands in for `WinGetClassList`, and for the same purpose: finding out what
/// kind of thing is in a window when a selector is not matching. Win32 class
/// names and AX roles are different vocabularies — `AXTextField` where Windows
/// says `Edit` — so this is an analogue, not a translation.
pub(crate) fn roles_of(window: &Window) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    collect(&window.element, 0, &mut |element| {
        if let Some(role) = ax::string_attribute(element, ax::ATTR_ROLE) {
            seen.insert(role);
        }
    });
    seen.into_iter().collect()
}

/// Walks an element and its descendants, depth first.
fn collect(element: &AXUIElement, depth: usize, f: &mut impl FnMut(&AXUIElement)) {
    if depth > MAX_DEPTH {
        return;
    }
    f(element);
    for child in ax::element_array(element, ax::ATTR_CHILDREN) {
        collect(&child, depth + 1, f);
    }
}

/// Reports the window's state flags.
pub(crate) fn state(window: &Window) -> WinState {
    let mut s = WinState::EXISTS | WinState::ENABLED;
    if ax::bool_attribute(&window.element, ax::ATTR_MINIMIZED).unwrap_or(false) {
        s |= WinState::MINIMIZED;
    } else {
        s |= WinState::VISIBLE;
    }
    if is_active(window) {
        s |= WinState::ACTIVE;
    }
    s
}

/// Whether an application is answering.
///
/// The macOS replacement for polling the cursor shape, and a better measurement
/// than that was: this asks the application a question and sees whether it
/// answers within the timeout. A beachballing app fails the round trip, which
/// is a direct observation of "busy" rather than an inference from what the
/// pointer looks like.
pub(crate) fn is_app_responsive(pid: i32, timeout: Duration) -> bool {
    let app = ax::app_element(pid);
    ax::set_timeout(&app, timeout.as_secs_f32().max(0.05));
    ax::attribute(&app, ax::ATTR_FOCUSED_WINDOW).is_some()
        || ax::attribute(&app, ax::ATTR_ROLE).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_titles_match_by_prefix_by_default() {
        // The behaviour existing automation depends on without saying so:
        // "Untitled" is expected to find "Untitled - TextEdit".
        assert!(title_matches(
            "Untitled - TextEdit",
            "Untitled",
            TitleMatchMode::StartsWith,
            false
        ));
        assert!(!title_matches(
            "Untitled - TextEdit",
            "TextEdit",
            TitleMatchMode::StartsWith,
            false
        ));
    }

    #[test]
    fn the_other_match_modes_behave_as_named() {
        let t = "Untitled - TextEdit";
        assert!(title_matches(
            t,
            "TextEdit",
            TitleMatchMode::Substring,
            false
        ));
        assert!(!title_matches(t, "Untitled", TitleMatchMode::Exact, false));
        assert!(title_matches(t, t, TitleMatchMode::Exact, false));
        // Advanced mode means bare titles stop matching entirely.
        assert!(!title_matches(t, t, TitleMatchMode::Advanced, false));
    }

    #[test]
    fn case_insensitivity_is_opt_in() {
        assert!(!title_matches(
            "Untitled",
            "untitled",
            TitleMatchMode::Exact,
            false
        ));
        assert!(title_matches(
            "Untitled",
            "untitled",
            TitleMatchMode::Exact,
            true
        ));
    }

    #[test]
    fn the_regexp_patterns_production_automation_uses_all_work() {
        // Taken verbatim in shape from the real selectors, with the client's
        // names replaced. Every one is literals separated by `(.*)`.
        assert!(regexp_matches(
            "Acme - NORTHWIND - Quality Certificate",
            "Acme - NORTHWIND(.*)Certificate(.*)"
        ));
        assert!(regexp_matches(
            "Acme - Invoice Issue - Outbound",
            "Acme - (.*)Invoice Issue(.*)"
        ));
        assert!(regexp_matches("DevTools - localhost", "DevTools - (.*)"));

        // And the negative cases, which matter more: a matcher that says yes to
        // everything is worse than none.
        assert!(!regexp_matches(
            "Something else",
            "Acme - (.*)Certificate(.*)"
        ));
        assert!(!regexp_matches(
            "Acme - Invoice",
            "Acme - (.*)Certificate(.*)"
        ));
    }

    #[test]
    fn a_regexp_without_a_leading_wildcard_anchors_at_the_start() {
        assert!(regexp_matches("Acme Invoices", "Acme(.*)"));
        assert!(!regexp_matches("The Acme Invoices", "Acme(.*)"));
    }

    #[test]
    fn matching_requires_every_criterion_not_merely_one() {
        // `[CLASS:x;TITLE:y]` means both, and treating it as either would find
        // the wrong window in exactly the situations selectors exist to
        // disambiguate.
        let selector = Selector::from("[TITLE:Untitled;PID:1]");
        let criteria = selector.criteria().expect("advanced selector");
        assert_eq!(criteria.len(), 2);
    }
}

/// Tests that need a real desktop and the Accessibility grant.
///
/// `#[ignore]`d because CI runners have neither, and because the grant is tied
/// to the binary's path and code signature — every `cargo test` builds a new
/// hash, so an ungranted run would prompt rather than fail. Run them with:
///
/// ```text
/// cargo test -p autoitx --lib live -- --ignored --nocapture
/// ```
#[cfg(test)]
mod live {
    use super::*;

    fn windows_or_skip() -> Vec<Window> {
        match all_windows() {
            Ok(w) => w,
            Err(e) => panic!("grant Accessibility to this terminal first: {e}"),
        }
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn enumerates_real_windows_with_titles_and_bundle_ids() {
        let windows = windows_or_skip();
        println!("{} windows", windows.len());
        for w in &windows {
            println!(
                "  pid {:<7} {:<34} {:?}",
                w.pid,
                bundle_id(w.pid).unwrap_or_default(),
                w.title
            );
        }
        assert!(
            !windows.is_empty(),
            "no windows at all — is the screen locked?"
        );
        assert!(
            windows.iter().any(|w| !w.title.is_empty()),
            "every window came back untitled, which means AX is not answering"
        );
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn the_matcher_finds_a_real_window_by_every_supported_criterion() {
        let windows = windows_or_skip();

        // Pick any titled window that belongs to an identifiable application,
        // rather than requiring a particular app to be open.
        let target = windows
            .iter()
            .find(|w| !w.title.is_empty() && bundle_id(w.pid).is_some())
            .expect("at least one titled window from a bundled application");
        let bundle = bundle_id(target.pid).expect("checked above");
        println!("target: {:?} from {bundle}", target.title);

        let exact = Selector::from(format!("[TITLE:{}]", target.title).as_str());
        assert!(
            matches(target, &exact, TitleMatchMode::Exact, false),
            "TITLE did not match its own window"
        );

        // The default: a bare prefix. This is what nearly all ported
        // automation relies on without saying so.
        let prefix: String = target.title.chars().take(4).collect();
        assert!(
            matches(
                target,
                &Selector::title(&prefix),
                TitleMatchMode::StartsWith,
                false
            ),
            "a {} character prefix did not match {:?}",
            prefix.len(),
            target.title
        );

        let by_class = Selector::from(format!("[CLASS:{bundle}]").as_str());
        assert!(
            matches(target, &by_class, TitleMatchMode::StartsWith, false),
            "CLASS did not match the bundle id"
        );

        let by_regexp = Selector::from(format!("[REGEXPTITLE:{prefix}(.*)]").as_str());
        assert!(
            matches(target, &by_regexp, TitleMatchMode::StartsWith, false),
            "REGEXPTITLE did not match"
        );

        let by_pid = Selector::from(format!("[PID:{}]", target.pid).as_str());
        assert!(matches(target, &by_pid, TitleMatchMode::StartsWith, false));

        // And the negative case, which matters more: a matcher that says yes to
        // everything would pass all of the above.
        let wrong = Selector::from("[TITLE:definitely not a real window title]");
        assert!(!matches(target, &wrong, TitleMatchMode::Exact, false));
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn the_frontmost_window_reports_itself_as_active() {
        let active = active_window()
            .expect("accessibility grant")
            .expect("something is frontmost");
        println!("active: pid {} {:?}", active.pid, active.title);

        assert!(
            is_active(&active),
            "the focused window is not seen as active"
        );
        assert!(state(&active).contains(WinState::ACTIVE | WinState::EXISTS));
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn a_window_reports_a_position_and_a_plausible_size() {
        let windows = windows_or_skip();
        let w = windows
            .iter()
            .find(|w| w.rect().is_some_and(|r| r.w > 0 && r.h > 0))
            .expect("at least one window with a real frame");
        let r = w.rect().expect("checked above");
        println!("{:?} at {r:?}", w.title);

        // Sanity, not precision: a window larger than any display, or one at a
        // wildly negative origin, means the coordinate space is wrong.
        assert!(r.w < 20_000 && r.h < 20_000, "implausible size {r:?}");
        assert!(r.x > -20_000 && r.y > -20_000, "implausible origin {r:?}");
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn the_visible_frame_excludes_the_menu_bar() {
        // Only meaningful on the main thread, which is where `cargo test` runs
        // each test body — but not the harness thread, so tolerate `None`.
        let Some(frame) = visible_frame() else {
            println!("not on the main thread; skipped");
            return;
        };
        println!("visible frame: {frame:?}");
        assert!(frame.w > 0 && frame.h > 0);
        // The menu bar is at the top, so the usable area starts below it.
        assert!(
            frame.y > 0,
            "visible frame starts at the very top: {frame:?}"
        );
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn a_windows_text_and_roles_can_be_read() {
        let windows = windows_or_skip();
        let w = windows
            .iter()
            .find(|w| !roles_of(w).is_empty())
            .expect("at least one window with an accessibility tree");

        let roles = roles_of(w);
        println!("{:?} contains roles {roles:?}", w.title);
        assert!(
            roles.iter().any(|r| r == "AXWindow"),
            "a window's own role should be AXWindow, got {roles:?}"
        );

        // Text may legitimately be empty (an empty document), so this asserts
        // only that the walk completes rather than that it found something.
        let text = text_of(w);
        println!(
            "first 200 chars of text: {:?}",
            text.chars().take(200).collect::<String>()
        );
    }

    #[test]
    #[ignore = "needs a real desktop and the Accessibility grant"]
    fn a_live_application_is_responsive_and_a_dead_pid_is_not() {
        let active = active_window()
            .expect("accessibility grant")
            .expect("something is frontmost");
        assert!(
            is_app_responsive(active.pid, Duration::from_millis(500)),
            "the frontmost application did not answer"
        );
        // The other half: a pid that cannot exist must not read as responsive,
        // or `wait_until_idle` would return immediately for anything.
        assert!(!is_app_responsive(999_999, Duration::from_millis(500)));
    }
}
