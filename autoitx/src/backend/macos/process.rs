//! Processes, the POSIX way.
//!
//! AutoIt names processes the way Windows does — `"chrome.exe"` — and matches
//! against the executable file name. macOS has the same notion under a
//! different spelling (`Google Chrome`), so [`find`] compares against the
//! `libproc` process name and tolerates a `.exe` suffix that only means
//! anything on the other platform. That way a selector table ported from
//! Windows keeps working where the names happen to line up, and fails visibly
//! where they do not.
//!
//! # Why `libproc` and not `NSWorkspace`
//!
//! `NSWorkspace::runningApplications` lists only applications with a GUI
//! presence. Automation regularly waits on a helper, an installer, or a command
//! line tool, none of which appear there. `proc_listpids` sees every process
//! the user can see.

#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

use std::time::Duration;

/// The buffer size `proc_name` writes into. Apple's own header caps the name
/// it returns well below this.
const NAME_CAP: usize = 256;

/// `PROC_ALL_PIDS` from `<libproc.h>`.
///
/// `libc` binds `proc_listpids` but not the selector constants that go with it,
/// so this is spelled out. The value is fixed ABI, not a version detail.
const PROC_ALL_PIDS: u32 = 1;

/// The name of a process, or `None` if it has exited or is not ours to see.
pub(crate) fn name_of(pid: i32) -> Option<String> {
    let mut buf = [0u8; NAME_CAP];
    // SAFETY: `buf` is a valid writable buffer of the length being passed.
    let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), NAME_CAP as u32) };
    if len <= 0 {
        return None;
    }
    let len = (len as usize).min(NAME_CAP);
    String::from_utf8(buf[..len].to_vec()).ok()
}

/// Every process id visible to this user.
pub(crate) fn all_pids() -> Vec<i32> {
    // Ask for the size first: the count moves between the two calls, so the
    // buffer is deliberately over-allocated rather than exactly sized.
    // SAFETY: passing a null buffer asks for the required size, per proc_listpids.
    let bytes = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if bytes <= 0 {
        return Vec::new();
    }

    let count = (bytes as usize / size_of::<i32>()) + 64;
    let mut pids = vec![0i32; count];
    // SAFETY: `pids` is a valid buffer of exactly the byte length passed.
    let written = unsafe {
        libc::proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr().cast(),
            (count * size_of::<i32>()) as i32,
        )
    };
    if written <= 0 {
        return Vec::new();
    }

    pids.truncate(written as usize / size_of::<i32>());
    // The kernel pads the tail with zeroes when processes exit mid-call.
    pids.retain(|p| *p > 0);
    pids
}

/// Whether a name refers to this process, allowing for Windows spelling.
///
/// `"chrome.exe"`, `"chrome"` and `"Chrome"` all match a process named
/// `Chrome`. The `.exe` is stripped because ported automation carries it and it
/// can never be part of a macOS process name.
fn name_matches(actual: &str, wanted: &str) -> bool {
    let wanted = wanted.strip_suffix(".exe").unwrap_or(wanted);
    actual.eq_ignore_ascii_case(wanted)
}

/// The process id for a name, or for a string that is already a pid.
///
/// Accepting both is AutoIt's behaviour, and automation relies on it: the same
/// argument is a name when a flow starts and a pid once it has one.
pub(crate) fn find(name_or_pid: &str) -> Option<i32> {
    if let Ok(pid) = name_or_pid.parse::<i32>() {
        // A numeric argument is only a pid if such a process exists; otherwise
        // fall through and treat it as a name, which is what a process
        // genuinely called "1234" would need.
        if pid > 0 && exists(pid) {
            return Some(pid);
        }
    }
    all_pids()
        .into_iter()
        .find(|pid| name_of(*pid).is_some_and(|n| name_matches(&n, name_or_pid)))
}

/// Whether a process id is live.
///
/// # Zombies count as gone
///
/// `kill(pid, 0)` — the usual liveness probe — succeeds for a process that has
/// exited but not yet been reaped by its parent. That is the normal state of
/// affairs right after automation closes something it launched itself, and
/// reporting it as running makes `process_wait_close` wait forever for a
/// process that is already dead.
///
/// The states below were measured rather than assumed, because the combination
/// that identifies a zombie is not the obvious one:
///
/// | state | `kill(pid, 0)` | `proc_pidinfo` |
/// |---|---|---|
/// | running, ours | 0 | full struct |
/// | running, another user's | `EPERM` | 0 bytes |
/// | exited, not reaped | 0 | **0 bytes** |
/// | reaped or never existed | `ESRCH` | 0 bytes |
///
/// So `proc_pidinfo` alone cannot tell a zombie from another user's process,
/// and `kill` alone cannot tell a zombie from a running one. Together they can.
pub(crate) fn exists(pid: i32) -> bool {
    // SAFETY: `kill` with signal 0 delivers nothing; it only runs the existence
    // and permission checks.
    if unsafe { libc::kill(pid, 0) } == 0 {
        // Ours, and signalable — so a missing info block means it has exited.
        return has_info(pid);
    }

    // Not signalable. `EPERM` means it exists and belongs to someone else,
    // which is still "running" as far as automation is concerned.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether the kernel will still describe this process.
///
/// Returns false for a zombie, which is what this is for.
fn has_info(pid: i32) -> bool {
    let size = size_of::<libc::proc_bsdinfo>();
    // SAFETY: `proc_bsdinfo` is a plain C struct of scalars and arrays, for
    // which an all-zero bit pattern is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };

    // SAFETY: the buffer is exactly the size declared, and matches the flavour
    // being requested.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            size as i32,
        )
    };

    written as usize == size && info.pbi_status != libc::SZOMB
}

/// Asks a process to exit, escalating to `SIGKILL` if it will not.
///
/// The escalation is what `ProcessClose` does on Windows and what automation
/// expects: an application that ignores the polite request still has to go, or
/// the next run finds its window still there.
pub(crate) fn close(pid: i32) -> bool {
    // SAFETY: sending a signal to a pid; failure is reported, not unsound.
    unsafe {
        if libc::kill(pid, libc::SIGTERM) != 0 {
            return false;
        }
    }

    // Give it a moment to handle the signal before insisting.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !exists(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // SAFETY: as above.
    unsafe { libc::kill(pid, libc::SIGKILL) == 0 }
}

/// Sets a process's scheduling priority.
///
/// AutoIt's scale is 0 (idle) to 5 (realtime); POSIX niceness runs the other
/// way, from 19 (lowest) to -20 (highest). The mapping is stated here rather
/// than left implicit, because the reversal is exactly the kind of thing that
/// silently does the opposite of what was asked.
pub(crate) fn set_priority(pid: i32, autoit_priority: i32) -> bool {
    let nice = match autoit_priority {
        0 => 19,  // Idle
        1 => 10,  // Below normal
        2 => 0,   // Normal
        3 => -5,  // Above normal
        4 => -10, // High
        5 => -20, // Realtime
        _ => return false,
    };
    // SAFETY: setpriority on a pid; a refusal is reported through the return.
    unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, nice) == 0 }
}

/// Whether this process is running as root.
///
/// The nearest thing to `IsAdmin`. Not the same question as "can this
/// automation control other applications" — that is the Accessibility grant,
/// which root does not confer.
pub(crate) fn is_root() -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_findable_by_its_own_pid_and_name() {
        let me = std::process::id() as i32;
        assert!(exists(me));

        let name = name_of(me).expect("our own process has a name");
        assert!(!name.is_empty());
        assert_eq!(find(&me.to_string()), Some(me));
    }

    #[test]
    fn a_pid_that_cannot_exist_is_absent() {
        // Above the default pid_max, so this is not a race with a real process.
        assert!(!exists(9_999_999));
        assert!(name_of(9_999_999).is_none());
    }

    #[test]
    fn the_process_list_contains_us_and_launchd() {
        let pids = all_pids();
        assert!(pids.contains(&(std::process::id() as i32)));
        // Pid 1 is always there, and always launchd on macOS.
        assert!(pids.contains(&1));
    }

    #[test]
    fn windows_spelling_is_accepted() {
        // The reason this exists: selector tables ported from Windows say
        // "chrome.exe", and refusing them would mean editing every call site.
        assert!(name_matches("Chrome", "chrome.exe"));
        assert!(name_matches("Chrome", "Chrome"));
        assert!(name_matches("Chrome", "CHROME"));
        assert!(!name_matches("Chrome", "Chromium"));
    }

    #[test]
    fn priorities_map_to_niceness_the_right_way_round() {
        // Guarding the reversal: AutoIt counts up for more urgent, POSIX counts
        // down. Setting our own priority to "below normal" must raise niceness.
        let me = std::process::id() as i32;
        assert!(set_priority(me, 1));

        // SAFETY: reading our own priority.
        let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, me as libc::id_t) };
        assert_eq!(nice, 10);

        // And an out-of-range value is refused rather than guessed at.
        assert!(!set_priority(me, 99));
    }

    #[test]
    fn a_child_process_can_be_found_and_closed() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id() as i32;

        assert!(exists(pid));
        assert_eq!(name_of(pid).as_deref(), Some("sleep"));

        assert!(close(pid));
        // Not reaped yet — this is the zombie case, and it has to read as gone
        // or a `process_wait_close` after a `run` would never return.
        assert!(!exists(pid));
        let _ = child.wait();
        assert!(!exists(pid));
    }

    #[test]
    fn another_users_process_reads_as_running() {
        // launchd is root's, so we cannot signal it — but it is emphatically
        // running, and reporting otherwise would make automation wait on a
        // process that will never appear.
        assert!(exists(1));
    }
}
