//! The macOS backend, driven against a real desktop.
//!
//! These are the acceptance tests for the native backend: everything the mock
//! DLL cannot answer, because there is no mock — the questions are about
//! whether Apple's frameworks do what this code believes they do.
//!
//! All `#[ignore]`d. CI runners have no Accessibility grant and no controllable
//! graphical session, and the grant is keyed to a binary's path and code
//! signature, so every rebuild produces a binary macOS has never seen. Run them
//! deliberately:
//!
//! ```text
//! cargo test -p autoitx --test macos_live -- --ignored --nocapture
//! ```
//!
//! # They open and close a real TextEdit
//!
//! Rather than driving whatever happens to be on screen, which would be both
//! unreliable and rude. Each test launches its own window and closes it, so a
//! failure leaves at most one stray Untitled document.

#![cfg(all(target_os = "macos", not(feature = "mock-loader")))]

use autoitx::options::{KeyMap, Options, ShowState};
use autoitx::{AutoIt, Selector, keys, recipes};
use std::time::Duration;

/// How long to wait for an application to appear or respond.
const PATIENCE: Duration = Duration::from_secs(10);

/// Every window title the backend can currently see.
///
/// Only used to make a failure legible: "the window never appeared" is not a
/// diagnosis, and the list distinguishes "the app did not launch" from "the app
/// launched and the selector is wrong" from "accessibility is not answering".
fn debug_titles() -> Vec<String> {
    let ai = match AutoIt::new() {
        Ok(ai) => ai,
        Err(e) => return vec![format!("<could not build backend: {e}>")],
    };
    // Any window at all, via a pattern that matches everything.
    let mut out = Vec::new();
    for probe in ["TextEdit", "autoitx", "Untitled"] {
        let s = Selector::from(format!("[REGEXPTITLE:(.*){probe}(.*)]").as_str());
        if let Ok(title) = ai.win_get_title(&s) {
            out.push(format!("{probe} -> {title:?}"));
        } else {
            out.push(format!("{probe} -> no match"));
        }
    }
    out
}

/// Opens a TextEdit window and closes it when the test ends.
struct Scratch {
    ai: AutoIt,
    selector: Selector,
}

impl Scratch {
    fn open() -> Self {
        let ai = AutoIt::builder()
            // So `{CTRLDOWN}c{CTRLUP}` means Copy here, as it would on Windows.
            .options(Options::default().with_key_map(KeyMap::PortableShortcuts))
            .build()
            .expect("grant Accessibility to your terminal first");

        // A file with a known name, so the window title is predictable and this
        // never picks up a document the user already had open.
        let path = std::env::temp_dir().join("autoitx-live-test.txt");
        std::fs::write(&path, "hello from autoitx").expect("write scratch file");
        ai.run(&format!("open -a TextEdit {}", path.display()), None)
            .expect("launch TextEdit");

        let selector = Selector::title("autoitx-live-test.txt");
        if !ai.win_wait(&selector, Some(PATIENCE)).expect("wait") {
            panic!(
                "TextEdit never opened a window titled autoitx-live-test.txt.\n\
                 Accessibility: {:?}\n\
                 windows seen: {:#?}",
                autoitx::ext::macos::check(autoitx::ext::macos::Permission::Accessibility),
                debug_titles(),
            );
        }
        assert!(
            ai.win_wait_activate(&selector, Some(PATIENCE))
                .expect("activate"),
            "the TextEdit window never took focus"
        );

        Self { ai, selector }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort: a panicking test should still not leave a window behind,
        // but a failure to clean up must not mask the original failure.
        let _ = self
            .ai
            .win_close_if_exists(&self.selector, Duration::from_secs(2));
    }
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn a_window_can_be_found_activated_and_measured() {
    let s = Scratch::open();

    assert!(s.ai.win_exists(&s.selector).unwrap());
    assert!(s.ai.win_active(&s.selector).unwrap());

    let title = s.ai.win_get_title(&s.selector).unwrap();
    println!("title: {title:?}");
    assert!(title.starts_with("autoitx-live-test.txt"));

    let pid = s.ai.win_get_process(&s.selector).unwrap();
    assert!(pid > 0);

    let rect = s.ai.win_get_pos(&s.selector).unwrap();
    println!("rect: {rect:?}");
    assert!(rect.w > 0 && rect.h > 0);
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn maximize_fills_the_screens_visible_area() {
    let s = Scratch::open();

    // Somewhere small first, so filling the screen is a visible change rather
    // than a coincidence.
    s.ai.win_move(&s.selector, autoitx::Rect::new(120, 120, 500, 400))
        .unwrap();
    let before = s.ai.win_get_pos(&s.selector).unwrap();
    println!("before: {before:?}");

    assert!(
        s.ai.win_set_state(&s.selector, ShowState::Maximize)
            .unwrap(),
        "maximize reported no window"
    );
    let after = s.ai.win_get_pos(&s.selector).unwrap();
    println!("after:  {after:?}");

    assert!(
        after.w > before.w && after.h > before.h,
        "the window did not grow: {before:?} -> {after:?}"
    );
    // The visible area excludes the menu bar, so a maximised window starts
    // below it — this is the AppKit bottom-left conversion being right.
    assert!(
        after.y > 0,
        "a maximised window should sit below the menu bar, got {after:?}"
    );
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn a_windows_state_reports_minimised_and_restored() {
    let s = Scratch::open();

    let state = s.ai.win_get_state(&s.selector).unwrap();
    println!("state: {state:?}");
    assert!(state.contains(autoitx::WinState::EXISTS | autoitx::WinState::VISIBLE));

    assert!(
        s.ai.win_set_state(&s.selector, ShowState::Minimize)
            .unwrap()
    );
    // Minimising is animated, so the flag lags the call.
    std::thread::sleep(Duration::from_millis(1200));
    assert!(
        s.ai.win_get_state(&s.selector)
            .unwrap()
            .contains(autoitx::WinState::MINIMIZED),
        "the window did not report itself minimised"
    );

    assert!(s.ai.win_set_state(&s.selector, ShowState::Restore).unwrap());
    std::thread::sleep(Duration::from_millis(1200));
    assert!(
        !s.ai
            .win_get_state(&s.selector)
            .unwrap()
            .contains(autoitx::WinState::MINIMIZED),
        "the window stayed minimised"
    );
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn read_screen_text_reads_what_is_in_the_document() {
    let s = Scratch::open();
    let session = s.ai.session();

    // Select the whole document and read it. This is the recipe end to end:
    // sequence number before, keystrokes, wait for the counter to move, read.
    let text =
        recipes::read_screen_text(&s.ai, keys!("{CTRLDOWN}a{CTRLUP}"), Duration::from_secs(5))
            .expect("read the document");
    drop(session);

    println!("read back: {text:?}");
    assert_eq!(text.trim(), "hello from autoitx");
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn the_clipboard_round_trips_and_its_sequence_number_moves() {
    let ai = AutoIt::new().expect("grant Accessibility first");

    // Put the user's clipboard back afterwards, whatever happens.
    let saved = ai.clip_get().unwrap_or_default();
    struct Restore<'a>(&'a AutoIt, String);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = self.0.clip_put(&self.1);
        }
    }
    let _restore = Restore(&ai, saved);

    let before = ai.clip_sequence().expect("macOS always has a change count");
    ai.clip_put("Ünïcödé ãõç — 1.234,56").unwrap();
    let after = ai.clip_sequence().expect("still there");

    assert_ne!(before, after, "the change count did not move");
    assert_eq!(ai.clip_get().unwrap(), "Ünïcödé ãõç — 1.234,56");
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn wait_until_idle_returns_for_a_responsive_application() {
    let _s = Scratch::open();
    let ai = AutoIt::new().expect("grant Accessibility first");

    // TextEdit is frontmost and doing nothing, so this should return almost
    // immediately rather than wait out the timeout.
    let started = std::time::Instant::now();
    recipes::wait_until_idle(&ai, Duration::from_secs(5)).expect("TextEdit is idle");
    let took = started.elapsed();
    println!("settled in {took:?}");
    assert!(
        took < Duration::from_secs(2),
        "took {took:?} to notice idle"
    );
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn a_deliberately_hung_application_is_not_reported_as_idle() {
    use autoitx::ext::macos as mac;

    // A process stopped with SIGSTOP is the closest reproducible thing to a
    // beachball: it is running, it owns windows, and it will not answer.
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawn");
    let pid = child.id() as i32;

    // SAFETY: stopping a child this test created.
    unsafe { libc::kill(pid, libc::SIGSTOP) };

    let responsive = mac::is_app_responsive(pid, Duration::from_millis(300));
    // SAFETY: as above.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let _ = child.wait();

    assert!(
        !responsive,
        "a stopped process answered an accessibility query"
    );
}

#[test]
#[ignore = "needs a real desktop and the Accessibility grant"]
fn a_pinned_handle_survives_a_dialog_stealing_focus() {
    // The bug this whole mechanism exists for: `[ACTIVE]` names whatever is
    // active *now*, so a close-and-wait sequence follows the focus onto the
    // next window and never finishes. A pinned handle keeps meaning one window.
    let s = Scratch::open();

    let handle = s.ai.win_get_handle(&s.selector).unwrap();
    assert_ne!(handle, 0);
    let pinned = Selector::handle(handle);

    assert!(
        s.ai.win_exists(&pinned).unwrap(),
        "the handle lost its window"
    );
    assert_eq!(
        s.ai.win_get_title(&pinned).unwrap(),
        s.ai.win_get_title(&s.selector).unwrap()
    );

    // Focus something else. The handle must still name the original window,
    // where `[ACTIVE]` would now name the other one.
    let active_title = s.ai.win_get_title(&Selector::active()).unwrap_or_default();
    println!(
        "active is {active_title:?}, pinned is still {:?}",
        s.ai.win_get_title(&pinned).unwrap()
    );
}
