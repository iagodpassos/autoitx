//! End-to-end FFI tests against the mock DLL — on whatever OS you are on.
//!
//! This is the load-bearing test of the whole project. It proves the Windows
//! marshalling layer works without Windows, without the licensed DLL, and
//! without a linker: `libloading` opens a `cdylib` the same way on every
//! platform, so a `.dylib` built from the same declarations as the real
//! bindings stands in for `AutoItX3_x64.dll` perfectly at the ABI level.
//!
//! What it can prove: argument order, argument *values*, UTF-16 round-tripping
//! (including non-ASCII), the `AU3_INTDEFAULT` sentinel, output-buffer
//! semantics, and that all 117 declared signatures are callable.
//!
//! What it cannot prove: that AutoIt *behaves* the way the safe layer expects.
//! That needs a real Windows machine, and is phase 2's acceptance criterion.
//!
//! Run with: `just test-mock`

#![cfg(feature = "mock-loader")]
#![allow(non_snake_case)]

use autoitx_sys::{AU3_INTDEFAULT, Au3, HWND, PCWSTR, POINT, PWSTR, RECT};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Locates the mock `cdylib` Cargo built for this test run.
///
/// It lands in one of two places depending on how the build was invoked, and
/// both are normal:
///
/// - `target/<profile>/deps/` when built as `autoitx-sys`'s dev-dependency,
///   which is what a plain `cargo test` does;
/// - `target/<profile>/` when built as a primary target, i.e.
///   `cargo build -p xtask-mock-dll`. Cargo only hard-links the artifact up to
///   the profile directory in that case.
///
/// Both are derived from the test binary's own location rather than assuming
/// `debug`, so `cargo test --release` works too.
fn mock_dylib() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary has a path");
    let deps = exe
        .parent()
        .expect("test binary lives in target/<profile>/deps/");
    let profile = deps.parent().expect("deps/ has a parent");

    let name = if cfg!(windows) {
        "xtask_mock_dll.dll"
    } else if cfg!(target_os = "macos") {
        "libxtask_mock_dll.dylib"
    } else {
        "libxtask_mock_dll.so"
    };

    let candidates = [deps.join(name), profile.join(name)];
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "mock library not found. Looked in:\n  {}\n  {}\n\
                 Run `cargo build -p xtask-mock-dll`, or `just test-mock`.",
                candidates[0].display(),
                candidates[1].display(),
            )
        })
}

/// Serialises the whole file.
///
/// The mock's call log is process-global state, and Cargo runs integration
/// tests in parallel threads of one process — so without this, tests wipe and
/// interleave each other's logs. Global (rather than thread-local) state in the
/// mock is the right trade: it keeps the mock able to record calls made from
/// worker threads, which the safe layer's `Session` tests will need.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The mock's out-of-band control surface, loaded from the same library.
///
/// Holds the file-wide lock for its whole lifetime, so simply constructing one
/// is enough to get exclusive access to the call log.
struct Mock {
    _guard: std::sync::MutexGuard<'static, ()>,
    au3: Au3,
    _lib: libloading::Library,
    reset: unsafe extern "system" fn(),
    call_count: unsafe extern "system" fn() -> i32,
    take_log: unsafe extern "system" fn(*mut u8, i32) -> i32,
    set_next_string: unsafe extern "system" fn(PCWSTR),
}

impl Mock {
    fn load() -> Self {
        // A panicking test poisons the lock; that must not cascade into every
        // subsequent test also failing for an unrelated reason.
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = mock_dylib();

        // Open for the MOCK_* controls first, so the log can be cleared
        // *before* `Au3::load_from` runs — otherwise the AU3_Init it emits
        // would be indistinguishable from a previous test's leftovers.
        // Opening the same library twice is fine: the OS refcounts it, so both
        // handles share one mapping, and therefore one call log.
        // SAFETY: the path names this workspace's own mock cdylib, built from
        // the same declaration list as the bindings being tested.
        let lib = unsafe { libloading::Library::new(&path) }.expect("open mock");

        // SAFETY: these symbols are declared in xtask-mock-dll with exactly
        // these signatures; dereferencing erases the borrow, and `lib` is moved
        // into the same struct so it outlives the pointers.
        let (reset, call_count, take_log, set_next_string) = unsafe {
            (
                *lib.get(b"MOCK_reset\0").expect("MOCK_reset"),
                *lib.get(b"MOCK_call_count\0").expect("MOCK_call_count"),
                *lib.get(b"MOCK_take_log\0").expect("MOCK_take_log"),
                *lib.get(b"MOCK_set_next_string\0")
                    .expect("MOCK_set_next_string"),
            )
        };
        let reset: unsafe extern "system" fn() = reset;
        // SAFETY: no arguments, resolved from the library just opened.
        unsafe { reset() };

        // SAFETY: as above — same library, asserted ABI-compatible.
        let au3 = unsafe { Au3::load_from(&path) }.expect("mock should load");

        Self {
            _guard: guard,
            au3,
            _lib: lib,
            reset,
            call_count,
            take_log,
            set_next_string,
        }
    }

    fn reset(&self) {
        // SAFETY: no arguments, resolved from the loaded mock.
        unsafe { (self.reset)() };
    }

    fn count(&self) -> i32 {
        // SAFETY: no arguments, resolved from the loaded mock.
        unsafe { (self.call_count)() }
    }

    /// The recorded calls, one `NAME(arg, ...)` line each.
    fn log(&self) -> String {
        // SAFETY: a null buffer is the documented "how much do I need?" query.
        let needed = unsafe { (self.take_log)(std::ptr::null_mut(), 0) };
        let mut buf = vec![0u8; needed.max(1) as usize];
        // SAFETY: `buf` is writable for exactly `needed` bytes.
        unsafe { (self.take_log)(buf.as_mut_ptr(), needed) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }

    fn script_string(&self, s: &str) {
        let w = wide(s);
        // SAFETY: `w` is NUL-terminated and outlives the call.
        unsafe { (self.set_next_string)(w.as_ptr()) };
    }
}

/// UTF-16, NUL-terminated — how every AutoItX string parameter is passed.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn loading_calls_au3_init_exactly_once() {
    let m = Mock::load();
    // `Mock::load` clears the log and then loads, so whatever is here came from
    // `Au3::load_from` itself. The real DLL needs AU3_Init to establish its
    // defaults table; the .NET wrapper does it in a static constructor, which
    // is why existing AutoIt code never calls it — so the Rust loader must.
    assert_eq!(m.log(), "AU3_Init()");
    assert_eq!(m.count(), 1, "AU3_Init should be called exactly once");
}

#[test]
fn non_ascii_strings_survive_the_utf16_round_trip() {
    let m = Mock::load();
    m.reset();

    // Real strings from the RPAs this crate is being extracted from. If UTF-16
    // marshalling is wrong, these are what break — and they break in
    // production, not in ASCII-only tests.
    for s in [
        "Order Entry",
        "Customer Notes",
        "Report Background Options",
        "R$ 1.234,56",
        "Acme - NORTHWIND — Quality Certificate",
    ] {
        let w = wide(s);
        // SAFETY: `w` is NUL-terminated and outlives the call.
        unsafe { (m.au3.AU3_Send)(w.as_ptr(), 0) };
    }

    let log = m.log();
    for s in [
        "Order Entry",
        "Customer Notes",
        "R$ 1.234,56",
    ] {
        assert!(log.contains(s), "{s:?} did not survive; log:\n{log}");
    }
}

#[test]
fn argument_order_is_preserved_across_a_ten_parameter_call() {
    let m = Mock::load();
    m.reset();

    // WinMenuSelectItem takes title, text, then eight menu items. Getting the
    // order wrong here would be invisible in a two-argument smoke test, and
    // would silently click the wrong menu entry in production.
    let args: Vec<Vec<u16>> = [
        "TITLE", "TEXT", "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8",
    ]
    .iter()
    .map(|s| wide(s))
    .collect();

    // SAFETY: all ten pointers are NUL-terminated and outlive the call.
    unsafe {
        (m.au3.AU3_WinMenuSelectItem)(
            args[0].as_ptr(),
            args[1].as_ptr(),
            args[2].as_ptr(),
            args[3].as_ptr(),
            args[4].as_ptr(),
            args[5].as_ptr(),
            args[6].as_ptr(),
            args[7].as_ptr(),
            args[8].as_ptr(),
            args[9].as_ptr(),
        )
    };

    assert_eq!(
        m.log(),
        r#"AU3_WinMenuSelectItem("TITLE", "TEXT", "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8")"#
    );
}

#[test]
fn int_default_sentinel_crosses_the_boundary_intact() {
    let m = Mock::load();
    m.reset();

    let btn = wide("left");
    // Speed omitted — the sentinel is how AutoItX spells "use the default",
    // and it is i32::MIN + 1, not i32::MIN. An off-by-one here would silently
    // become a real speed value.
    // SAFETY: `btn` is NUL-terminated and outlives the call.
    unsafe { (m.au3.AU3_MouseClick)(btn.as_ptr(), 170, 180, 1, AU3_INTDEFAULT) };

    assert_eq!(
        m.log(),
        r#"AU3_MouseClick("left", 170, 180, 1, INTDEFAULT)"#
    );
}

#[test]
fn output_buffers_are_filled_and_truncated_like_the_real_dll() {
    let m = Mock::load();
    m.reset();
    m.script_string("Order Entry");

    // Roomy buffer: the whole value comes back.
    let mut buf = vec![0u16; 64];
    // SAFETY: `buf` is writable for 64 wide chars, which is what we pass.
    unsafe { (m.au3.AU3_ClipGet)(buf.as_mut_ptr(), 64) };
    assert_eq!(from_wide(&buf), "Order Entry");

    // Tight buffer: AutoItX truncates and still NUL-terminates, and gives no
    // hint that it did. That silence is exactly why the safe layer has to grow
    // and retry rather than trust one call.
    m.script_string("Order Entry");
    let mut small = vec![0u16; 8];
    // SAFETY: `small` is writable for 8 wide chars.
    unsafe { (m.au3.AU3_ClipGet)(small.as_mut_ptr(), 8) };
    let got = from_wide(&small);
    assert_eq!(got.chars().count(), 7, "must fill cap-1 and NUL-terminate");
    assert!("Order Entry".starts_with(&got), "got {got:?}");
}

#[test]
fn out_pointer_structs_are_writable() {
    let m = Mock::load();
    m.reset();

    let title = wide("[ACTIVE]");
    let text = wide("");
    let mut rect = RECT::default();
    // SAFETY: pointers are valid and outlive the call.
    unsafe { (m.au3.AU3_WinGetPos)(title.as_ptr(), text.as_ptr(), &raw mut rect) };

    let mut point = POINT::default();
    // SAFETY: as above.
    unsafe { (m.au3.AU3_MouseGetPos)(&raw mut point) };

    let log = m.log();
    assert!(
        log.contains(r#"AU3_WinGetPos("[ACTIVE]", "", rect:out)"#),
        "{log}"
    );
    assert!(log.contains("AU3_MouseGetPos(point:out)"), "{log}");
}

#[test]
fn handles_cross_as_opaque_pointers() {
    let m = Mock::load();
    m.reset();

    let h = 0xdead_beefusize as HWND;
    // SAFETY: the mock only formats the pointer; it never dereferences it.
    unsafe { (m.au3.AU3_WinActivateByHandle)(h) };

    assert_eq!(m.log(), "AU3_WinActivateByHandle(hwnd:0xdeadbeef)");
}

// ---------------------------------------------------------------------------
// The exhaustive sweep
// ---------------------------------------------------------------------------

/// A safe placeholder for each type in the ABI.
trait Dummy {
    fn dummy() -> Self;
}

impl Dummy for i32 {
    fn dummy() -> Self {
        0
    }
}
impl Dummy for PCWSTR {
    fn dummy() -> Self {
        std::ptr::null()
    }
}
impl Dummy for PWSTR {
    fn dummy() -> Self {
        std::ptr::null_mut()
    }
}
impl Dummy for HWND {
    fn dummy() -> Self {
        std::ptr::null_mut()
    }
}
impl Dummy for *mut RECT {
    fn dummy() -> Self {
        std::ptr::null_mut()
    }
}
impl Dummy for *mut POINT {
    fn dummy() -> Self {
        std::ptr::null_mut()
    }
}

macro_rules! sweep {
    ($(
        $(#[$meta:meta])*
        fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? ;
    )+) => {
        /// Calls every declared function once and returns how many were called.
        fn call_every_function(au3: &Au3) -> usize {
            let mut n = 0usize;
            $(
                // SAFETY: every pointer argument is null, and the mock checks
                // for null before dereferencing anything.
                let _ = unsafe { (au3.$name)($(<$ty as Dummy>::dummy()),*) };
                n += 1;
            )+
            n
        }
    };
}

autoitx_sys::au3_functions!(sweep);

#[test]
fn every_declared_signature_is_callable() {
    let m = Mock::load();
    m.reset();

    let called = call_every_function(&m.au3);

    // If a declared signature disagreed with the mock's — a wrong argument
    // count, a mismatched type — this would corrupt the stack or crash rather
    // than return cleanly.
    assert_eq!(called, 117, "expected to call all 117 functions");
    assert_eq!(m.count(), 117, "the mock should have recorded all 117");

    // And every one of them by the right name.
    let log = m.log();
    for name in Au3::SYMBOLS {
        assert!(log.contains(name), "{name} was never recorded");
    }
}
