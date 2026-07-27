//! The safe layer, driven against the mock DLL.
//!
//! Everything here runs on macOS as an ordinary `cargo test`. It proves what a
//! Windows machine would otherwise be needed for: that the safe API turns into
//! exactly the AU3 calls intended, with the right arguments, in the right
//! order.
//!
//! What it deliberately does *not* prove is that AutoIt then behaves as
//! expected — that a real window activates, that a real Chrome console takes
//! the keystrokes. Only Windows can answer that.

#![cfg(feature = "mock-loader")]

mod common;

use autoitx::options::ShowState;
use autoitx::{Keys, Point, Selector, keys, recipes};
use common::Harness;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Keystrokes
// ---------------------------------------------------------------------------

#[test]
fn send_passes_the_sequence_through_verbatim() {
    let h = Harness::new();
    h.ai.send(keys!("{CTRLDOWN}{SHIFTDOWN}j{SHIFTUP}{CTRLUP}"))
        .unwrap();
    assert_eq!(
        h.log(),
        r#"AU3_Send("{CTRLDOWN}{SHIFTDOWN}j{SHIFTUP}{CTRLUP}", 0)"#
    );
}

#[test]
fn send_text_escapes_before_it_reaches_the_dll() {
    let h = Harness::new();

    // The exact bug this crate exists to prevent: data that contains send
    // syntax. It must arrive at AU3_Send already neutralised.
    h.ai.send_text("Macro{123}!").unwrap();

    assert_eq!(h.log(), r#"AU3_Send("Macro{{}123{}}{!}", 0)"#);
}

#[test]
fn send_never_uses_raw_mode() {
    // Mode 1 would send `{ENTER}` as eight literal characters. `Keys::text`
    // already handles literals by escaping, so raw mode would only ever be a
    // way to get the two mechanisms fighting.
    let h = Harness::new();
    h.ai.send_text("plain").unwrap();
    assert!(h.log().ends_with(", 0)"), "{}", h.log());
}

#[test]
fn non_ascii_survives_the_whole_stack() {
    let h = Harness::new();
    h.ai.send_text("Ünïcödé ãõç — 1.234,56").unwrap();
    assert!(
        h.log().contains("Ünïcödé ãõç — 1.234,56"),
        "{}",
        h.log()
    );
}

// ---------------------------------------------------------------------------
// Selectors
// ---------------------------------------------------------------------------

#[test]
fn selectors_reach_the_dll_in_autoits_own_syntax() {
    let h = Harness::new();
    let sel = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]");
    h.ai.win_exists(&sel).unwrap();

    assert_eq!(
        h.log(),
        r#"AU3_WinExists("[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]", "")"#
    );
}

#[test]
fn a_bare_title_is_sent_as_a_bare_title() {
    // It must NOT become `[TITLE:...]`: a bare title uses WinTitleMatchMode
    // (prefix by default), and advanced syntax does not.
    let h = Harness::new();
    h.ai.win_exists(&Selector::title("Order Entry"))
        .unwrap();
    assert_eq!(h.log(), r#"AU3_WinExists("Order Entry", "")"#);
}

#[test]
fn the_window_text_parameter_is_always_empty() {
    // No production call site uses it, and AutoItX wants an empty string
    // rather than null.
    let h = Harness::new();
    h.ai.win_activate(&Selector::active()).unwrap();
    assert_eq!(h.log(), r#"AU3_WinActivate("[ACTIVE]", "")"#);
}

// ---------------------------------------------------------------------------
// Output buffers
// ---------------------------------------------------------------------------

#[test]
fn a_short_value_comes_back_in_one_call() {
    let h = Harness::new();
    h.script_string("none");
    assert_eq!(h.ai.clip_get().unwrap(), "none");
    assert_eq!(h.calls().len(), 1, "should not have needed a retry");
}

#[test]
fn a_value_larger_than_the_buffer_is_retried_until_it_fits() {
    let h = Harness::new();

    // win_get_title starts at 1024 wide chars. A 3000-char title forces the
    // grow-and-retry path — the one thing AutoItX gives no signal for, and so
    // the one thing most likely to silently truncate.
    let long = "ã".repeat(3000);
    h.script_string(&long);

    let got = h.ai.win_get_title(&Selector::active()).unwrap();

    assert_eq!(got.chars().count(), 3000, "value was truncated");
    assert_eq!(got, long);
    assert!(
        h.calls().len() > 1,
        "expected a retry, got {} call(s)",
        h.calls().len()
    );
}

#[test]
fn an_empty_clipboard_is_not_an_error() {
    // AutoItX sets its error flag when the clipboard holds no text. Every
    // existing call site treats that as an ordinary empty value, and an empty
    // clipboard genuinely is one.
    let h = Harness::new();
    h.script_string("");
    assert_eq!(h.ai.clip_get().unwrap(), "");
}

#[test]
fn interior_nul_is_rejected_rather_than_truncating_silently() {
    let h = Harness::new();
    let err = h.ai.clip_put("before\0after").unwrap_err();
    assert!(
        matches!(err, autoitx::Error::InteriorNul { at: 6, .. }),
        "{err:?}"
    );
    assert!(h.calls().is_empty(), "must not have called the DLL");
}

// ---------------------------------------------------------------------------
// Composed operations
// ---------------------------------------------------------------------------

#[test]
fn win_wait_activate_checks_before_activating() {
    let h = Harness::new();
    let sel = Selector::from("[CLASS:Chrome_WidgetWin_1]");

    // Not active (mock returns 0), so it should activate and then wait.
    h.ai.win_wait_activate(&sel, Some(Duration::from_secs(30)))
        .unwrap();

    assert_eq!(
        h.call_names(),
        ["AU3_WinActive", "AU3_WinActivate", "AU3_WinWaitActive"]
    );
    assert!(
        h.calls()[2].ends_with(", 30)"),
        "timeout not passed through: {:?}",
        h.calls()[2]
    );
}

#[test]
fn win_wait_activate_skips_activation_when_already_focused() {
    let h = Harness::new();
    h.script_int(1); // WinActive returns non-zero

    h.ai.win_wait_activate(&Selector::active(), None).unwrap();

    assert_eq!(
        h.call_names(),
        ["AU3_WinActive", "AU3_WinWaitActive"],
        "should not have re-activated an already-focused window"
    );
}

#[test]
fn no_timeout_is_encoded_as_autoits_wait_forever() {
    let h = Harness::new();
    h.ai.win_wait_close(&Selector::active(), None).unwrap();
    assert!(h.log().ends_with(", 0)"), "{}", h.log());
}

#[test]
fn a_sub_second_timeout_does_not_round_down_to_wait_forever() {
    // AutoIt reads 0 as "no timeout", so truncating 500ms to 0 would turn a
    // half-second wait into an unbounded one.
    let h = Harness::new();
    h.ai.win_wait_close(&Selector::active(), Some(Duration::from_millis(500)))
        .unwrap();
    assert!(h.log().ends_with(", 1)"), "{}", h.log());
}

#[test]
fn maximize_uses_the_sw_maximize_value() {
    let h = Harness::new();
    h.ai.maximize(&Selector::from("[CLASS:Chrome_WidgetWin_1]"))
        .unwrap();
    assert_eq!(
        h.log(),
        r#"AU3_WinSetState("[CLASS:Chrome_WidgetWin_1]", "", 3)"#
    );
    assert_eq!(ShowState::Maximize as i32, 3);
}

#[test]
fn close_if_exists_does_nothing_when_no_window_matches() {
    let h = Harness::new();
    // Mock returns a null handle from WinGetHandle: nothing matched.
    let closed =
        h.ai.win_close_if_exists(&Selector::active(), Duration::from_secs(60))
            .unwrap();

    assert!(!closed);
    assert_eq!(
        h.call_names(),
        ["AU3_WinGetHandle"],
        "only the identity lookup"
    );
}

#[test]
fn close_if_exists_pins_the_window_by_handle_before_escalating() {
    // Regression, found on a real desktop.
    //
    // The whole sequence must target the one window the caller meant, not
    // "whatever matches now". With `[ACTIVE]` the difference is stark: the app
    // pops a save dialog, which becomes active; the process is killed, and some
    // other application becomes active. A wait for `[ACTIVE]` to vanish never
    // finishes — there is always an active window — so a successful close is
    // reported as a timeout.
    let h = Harness::new();
    h.script_ints(&[
        0x4_0B1E, // WinGetHandle -> pin this window
        1,        // WinClose     -> accepted
        0,        // WinWaitClose -> still there
        4242,     // WinGetProcess
        1,        // ProcessClose
        1,        // WinWaitClose -> gone
    ]);

    let closed =
        h.ai.win_close_if_exists(&Selector::active(), Duration::from_millis(50))
            .unwrap();

    assert!(closed);
    assert_eq!(
        h.call_names(),
        [
            "AU3_WinGetHandle",
            "AU3_WinClose",
            "AU3_WinWaitClose",
            "AU3_WinGetProcess",
            "AU3_ProcessClose",
            "AU3_WinWaitClose",
        ],
        "escalation order changed"
    );

    // Every window operation after the lookup must address the handle, never
    // `[ACTIVE]`. `ProcessClose` is exempt: it takes a process id.
    for call in h.calls().iter().skip(1) {
        if call.starts_with("AU3_ProcessClose") {
            continue;
        }
        assert!(
            call.contains("[HANDLE:40b1e]"),
            "call still uses an unstable selector: {call}"
        );
    }
    // `[ACTIVE]` appears exactly once — in the lookup that resolves it. That is
    // the point of the whole design: name the unstable thing one time, then
    // stop.
    assert_eq!(
        h.calls().iter().filter(|c| c.contains("ACTIVE")).count(),
        1,
        "[ACTIVE] should be resolved once and never used again:\n{}",
        h.log()
    );
    assert!(h.calls()[0].contains("[ACTIVE]"), "{}", h.calls()[0]);

    assert!(
        h.calls()[4].contains("4242"),
        "killed the wrong process: {:?}",
        h.calls()[4]
    );
}

#[test]
fn close_if_exists_stops_early_when_the_window_closes_politely() {
    let h = Harness::new();
    h.script_ints(&[0x4_0B1E, 1, 1]); // handle, close accepted, gone

    let closed =
        h.ai.win_close_if_exists(&Selector::active(), Duration::from_secs(1))
            .unwrap();

    assert!(closed);
    assert_eq!(
        h.call_names(),
        ["AU3_WinGetHandle", "AU3_WinClose", "AU3_WinWaitClose"],
        "should not have escalated"
    );
}

#[test]
fn close_if_exists_reports_a_process_that_will_not_die() {
    let h = Harness::new();
    // Never closes, even after the kill. The .NET version waits forever here;
    // an unkillable process is something to report, not to hang on.
    h.script_ints(&[0x4_0B1E, 1, 0, 4242, 1, 0]);

    let err =
        h.ai.win_close_if_exists(&Selector::active(), Duration::from_millis(50))
            .unwrap_err();

    assert!(
        matches!(
            err,
            autoitx::Error::Timeout {
                operation: "win_close_if_exists",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn win_get_handle_reports_a_missing_window() {
    let h = Harness::new(); // mock returns a null handle
    let err = h.ai.win_get_handle(&Selector::active()).unwrap_err();
    assert!(
        matches!(err, autoitx::Error::WindowNotFound { .. }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// Mouse and recipes
// ---------------------------------------------------------------------------

#[test]
fn mouse_click_defaults_to_one_left_click_at_autoits_speed() {
    let h = Harness::new();
    h.script_int(1); // AU3_MouseClick reports success
    h.ai.mouse_click(Point::new(1350, 290)).unwrap();
    // INTDEFAULT for speed, rather than hard-coding 10: if AutoIt ever changes
    // its default, this follows it.
    assert_eq!(
        h.log(),
        r#"AU3_MouseClick("left", 1350, 290, 1, INTDEFAULT)"#
    );
}

#[test]
fn a_failed_click_is_an_error_rather_than_silence() {
    // AutoIt returns 0 when it could not click — invalid coordinates, usually.
    // The .NET code ignores that return value entirely and carries on as if the
    // click landed, which is how automation ends up several steps deep in the
    // wrong state before anything looks wrong.
    let h = Harness::new();
    let err = h.ai.mouse_click(Point::new(-1, -1)).unwrap_err();
    assert!(
        matches!(
            err,
            autoitx::Error::AutoItFailed {
                func: "AU3_MouseClick",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn click_in_window_anchors_to_the_window_rather_than_the_screen() {
    let h = Harness::new();
    let sel = Selector::from("[TITLE:Acme ERP;CLASS:ui60Modal_W32]");
    h.script_ints(&[1, 1]); // WinGetPos found it; the click succeeded

    // The mock leaves the RECT zeroed, so the window origin is (0,0) and the
    // click lands at the raw offset. What matters is that WinGetPos runs first
    // and the offset is added to its origin, rather than being used as an
    // absolute screen coordinate.
    recipes::click_in_window(&h.ai, &sel, 600, 420).unwrap();

    let calls = h.calls();
    assert_eq!(calls.len(), 2, "{calls:#?}");
    assert!(calls[0].starts_with("AU3_WinGetPos("), "{calls:#?}");
    assert_eq!(
        calls[1],
        r#"AU3_MouseClick("left", 600, 420, 1, INTDEFAULT)"#
    );
}

#[test]
fn click_in_window_fails_loudly_when_the_window_is_gone() {
    // Clicking at a stale coordinate because the window vanished is how
    // automation ends up pressing whatever is underneath.
    let h = Harness::new();
    h.script_error(1); // AutoIt sets @error = 1 when no window matches
    let err = recipes::click_in_window(&h.ai, &Selector::active(), 10, 10).unwrap_err();
    assert!(
        matches!(err, autoitx::Error::WindowNotFound { .. }),
        "{err:?}"
    );
    assert_eq!(h.call_names(), ["AU3_WinGetPos"], "must not have clicked");
}

#[test]
fn win_get_pos_trusts_the_error_flag_not_the_integer_return() {
    // Regression, found on a real desktop.
    //
    // `WinGetPos` fills a RECT and reports failure through @error; its integer
    // return is unspecified. Treating a 0 return as "not found" rejected
    // windows that plainly existed — `win_get_title` answered for the very same
    // selector one line earlier.
    let h = Harness::new();

    // The mock returns 0 from every call and leaves @error clear: exactly the
    // shape that used to break — found, but returning 0.
    let rect =
        h.ai.win_get_pos(&Selector::active())
            .expect("a clear error flag means the window was found");
    assert_eq!(rect, autoitx::Rect::new(0, 0, 0, 0));

    // With the flag set, it is a genuine miss.
    h.script_error(1);
    assert!(h.ai.win_get_pos(&Selector::active()).is_err());
}

#[test]
fn win_get_process_rejects_the_minus_one_sentinel() {
    // Regression, measured against a real desktop.
    //
    // `WinGetProcess` signals "no window" with `(DWORD)-1`, not 0. Returning
    // that as a process id had teeth: `win_close_if_exists` checks `pid != 0`
    // before terminating, so 4294967295 sailed straight through and the robot
    // would try to kill it.
    let h = Harness::new();
    h.script_int(-1); // the mock casts i32 -> DWORD, giving 0xFFFFFFFF

    let err = h.ai.win_get_process(&Selector::active()).unwrap_err();
    assert!(
        matches!(err, autoitx::Error::WindowNotFound { .. }),
        "expected WindowNotFound, got {err:?}"
    );
}

#[test]
fn close_if_exists_never_tries_to_kill_the_sentinel_pid() {
    // The failure this prevents, end to end: window exists, refuses to close,
    // and then WinGetProcess cannot find it.
    let h = Harness::new();
    h.script_ints(&[
        1,  // WinExists    -> yes
        1,  // WinClose     -> accepted
        0,  // WinWaitClose -> still there
        -1, // WinGetProcess -> (DWORD)-1, no window
    ]);

    let err =
        h.ai.win_close_if_exists(&Selector::active(), Duration::from_millis(50))
            .unwrap_err();

    assert!(
        matches!(err, autoitx::Error::WindowNotFound { .. }),
        "{err:?}"
    );
    assert!(
        !h.call_names().contains(&"AU3_ProcessClose".to_owned()),
        "must not have attempted a kill: {:#?}",
        h.call_names()
    );
}

#[test]
fn win_get_title_cannot_distinguish_missing_from_untitled() {
    // Measured: AutoItX leaves the error flag clear in both cases, so this is
    // documented behaviour rather than a wrapper that forgot to check. The test
    // exists so the day someone "fixes" it, they see it was deliberate.
    let h = Harness::new();
    h.script_string("");
    assert_eq!(h.ai.win_get_title(&Selector::active()).unwrap(), "");

    h.script_string("");
    h.script_error(0); // exactly what the DLL reports for a missing window
    assert_eq!(h.ai.win_get_title(&Selector::active()).unwrap(), "");
}

#[test]
fn win_get_class_list_does_report_a_missing_window() {
    // And this one, shaped identically, *does* set the flag. The inconsistency
    // is AutoItX's.
    let h = Harness::new();
    h.script_string("");
    let err = h.ai.win_get_class_list(&Selector::active()).unwrap_err();
    assert!(
        matches!(err, autoitx::Error::WindowNotFound { .. }),
        "{err:?}"
    );
}

#[test]
fn win_get_class_list_splits_on_newlines() {
    let h = Harness::new();
    h.script_string("Static\nEdit\nButton");
    assert_eq!(
        h.ai.win_get_class_list(&Selector::active()).unwrap(),
        ["Static", "Edit", "Button"]
    );
}

#[test]
fn process_exists_actually_returns_a_process_id() {
    // AutoIt's `ProcessExists` is misnamed: the return is the pid, not 1.
    let h = Harness::new();
    h.script_int(4720);
    assert_eq!(h.ai.process_id("notepad.exe").unwrap(), Some(4720));
    assert!(h.ai.process_exists("notepad.exe").unwrap());

    h.script_int(0);
    assert_eq!(h.ai.process_id("__nao_existe__.exe").unwrap(), None);
    assert!(!h.ai.process_exists("__nao_existe__.exe").unwrap());
}

#[test]
fn win_get_state_reports_through_the_error_flag() {
    // Measured: the return carries the state bits, and 0 — nothing set — is a
    // legitimate value, so only the error flag can mean "not found".
    let h = Harness::new();
    h.script_int(15); // EXISTS | VISIBLE | ENABLED | ACTIVE

    let state = h.ai.win_get_state(&Selector::active()).unwrap();
    assert!(state.contains(autoitx::WinState::EXISTS));
    assert!(state.contains(autoitx::WinState::ACTIVE));
    assert!(!state.contains(autoitx::WinState::MINIMIZED));

    h.script_error(1);
    assert!(h.ai.win_get_state(&Selector::active()).is_err());
}

#[test]
fn activate_and_set_state_report_whether_the_window_was_found() {
    // Measured: both return a plain 1/0, despite AutoIt's *script* docs
    // describing WinActivate as returning a window handle. Neither is an error
    // — activation is normally followed by a wait, which is the real check.
    let h = Harness::new();

    h.script_ints(&[0, 1, 0, 1]);
    assert!(!h.ai.win_activate(&Selector::active()).unwrap());
    assert!(h.ai.win_activate(&Selector::active()).unwrap());
    assert!(!h.ai.maximize(&Selector::active()).unwrap());
    assert!(h.ai.maximize(&Selector::active()).unwrap());
}

// The mock cannot stand in for the clipboard sequence number, and that is a
// property of the design rather than a gap: `clip_sequence` deliberately asks
// the operating system, not the DLL, because the whole point is to observe
// writes made by *other* processes. So these two tests assert whichever path
// actually applies:
//
//   - where no counter exists (macOS, Linux), the settle fallback runs and the
//     full call sequence is observable;
//   - where one does (Windows), the mock's clipboard is not the real one, so
//     the counter never moves — and the resulting timeout is itself proof that
//     the sequence path is in use, which is the more valuable assertion.

#[test]
fn read_screen_text_selects_copies_then_reads() {
    let h = Harness::new();
    h.script_string("115597/1");

    let resultado = recipes::read_screen_text(
        &h.ai,
        keys!("{END}{SHIFTDOWN}{HOME}{SHIFTUP}"),
        Duration::from_millis(300),
    );

    // Either way, the keystrokes must have gone out in the right order before
    // anything was read.
    let calls = h.call_names();
    assert_eq!(
        &calls[..2],
        ["AU3_Send", "AU3_Send"],
        "select then copy, before any read: {calls:#?}"
    );
    assert_eq!(
        h.calls()[0],
        r#"AU3_Send("{END}{SHIFTDOWN}{HOME}{SHIFTUP}", 0)"#
    );
    assert_eq!(h.calls()[1], r#"AU3_Send("{CTRLDOWN}c{CTRLUP}", 0)"#);

    match h.ai.clip_sequence() {
        None => {
            assert_eq!(resultado.unwrap(), "115597/1");
            assert_eq!(calls[2], "AU3_ClipGet", "must read only after copying");
        }
        Some(_) => {
            let err = resultado.expect_err(
                "with a real sequence counter and a mock clipboard, the counter \
                 cannot move — this must time out rather than return stale text",
            );
            assert!(
                matches!(
                    err,
                    autoitx::Error::Timeout {
                        operation: "read_screen_text",
                        ..
                    }
                ),
                "{err:?}"
            );
        }
    }
}

#[test]
fn read_screen_text_never_writes_to_the_clipboard() {
    // The idiom it replaces puts a sentinel there first, which clobbers
    // whatever the user had. Waiting on the sequence number does not need to.
    let h = Harness::new();
    h.script_string("qualquer coisa");

    // Succeeds or times out depending on the platform — see the note above.
    // Either way it must not have touched the clipboard, which is the claim
    // being tested.
    let _ = recipes::read_screen_text(&h.ai, keys!("{END}"), Duration::from_millis(300));

    assert!(
        !h.call_names().contains(&"AU3_ClipPut".to_owned()),
        "must not have written to the clipboard: {:#?}",
        h.call_names()
    );
}

#[test]
fn wait_until_idle_gives_up_instead_of_hanging_forever() {
    let h = Harness::new();
    // Cursor 15 is the hourglass: never idle. The hand-written version of this
    // loop in production AutoIt code has no timeout at all.
    h.script_int(15);

    let err = recipes::wait_until_idle(&h.ai, Duration::from_millis(300)).unwrap_err();
    assert!(
        matches!(
            err,
            autoitx::Error::Timeout {
                operation: "wait_until_idle",
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn wait_until_idle_accepts_the_ibeam_cursor_too() {
    let h = Harness::new();
    h.script_int(5); // I-beam: idle, over a text field
    recipes::wait_until_idle(&h.ai, Duration::from_secs(1)).unwrap();
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[test]
fn a_session_can_make_nested_calls_without_deadlocking() {
    // The lock is reentrant precisely so this works: `Session` holds it, and
    // every call inside takes it again.
    let h = Harness::new();
    let s = h.ai.session();
    s.send(keys!("{TAB}")).unwrap();
    s.clip_put("x").unwrap();
    s.win_exists(&Selector::active()).unwrap();
    drop(s);

    assert_eq!(h.calls().len(), 3);
}

#[test]
fn keys_and_text_compose() {
    let h = Harness::new();
    // The WMS login shape: credentials as data, navigation as commands.
    let seq = Keys::text("74").then(keys!("{TAB}"));
    h.ai.send(seq).unwrap();
    assert_eq!(h.log(), r#"AU3_Send("74{TAB}", 0)"#);
}

#[test]
fn a_long_title_needing_growth_still_round_trips_non_ascii() {
    // The retry path re-reads the buffer from scratch; a bug there would show
    // up as mangled multi-byte characters rather than as a length mismatch.
    let h = Harness::new();
    let long = "Ünïcödé ãõç — ".repeat(300);
    h.script_string(&long);
    assert_eq!(h.ai.win_get_title(&Selector::active()).unwrap(), long);
}
