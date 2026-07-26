//! Shared harness for driving the safe layer against the mock DLL.

use autoitx::AutoIt;
use std::path::PathBuf;

/// Locates the mock `cdylib` Cargo built for this test run.
///
/// It lands in `target/<profile>/deps/` when built as a dev-dependency, and in
/// `target/<profile>/` when built as a primary target.
pub fn mock_dylib() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary has a path");
    let deps = exe.parent().expect("test binary lives in deps/");
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
                 Run `cargo build -p xtask-mock-dll`.",
                candidates[0].display(),
                candidates[1].display(),
            )
        })
}

/// Serialises the file: the mock's call log is process-global, and Cargo runs
/// integration tests in parallel threads of one process.
static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// An [`AutoIt`] wired to the mock, plus the mock's control surface.
pub struct Harness {
    #[expect(dead_code, reason = "held for its Drop; the lock is the point")]
    guard: std::sync::MutexGuard<'static, ()>,
    /// The automation handle under test.
    pub ai: AutoIt,
    #[expect(dead_code, reason = "must outlive the function pointers below")]
    lib: libloading::Library,
    // No `reset` here: `new()` clears the log itself, so every test starts
    // clean without having to remember to.
    take_log: unsafe extern "system" fn(*mut u8, i32) -> i32,
    set_next_string: unsafe extern "system" fn(*const u16),
    set_next_int: unsafe extern "system" fn(i32),
    push_int: unsafe extern "system" fn(i32),
}

impl Harness {
    /// Loads the mock and builds an `AutoIt` against it, with a clean log.
    pub fn new() -> Self {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = mock_dylib();

        // SAFETY: this workspace's own mock cdylib, built from the same
        // declaration list as the real bindings.
        let lib = unsafe { libloading::Library::new(&path) }.expect("open mock");

        // SAFETY: these symbols are declared in xtask-mock-dll with exactly
        // these signatures; `lib` is moved into the same struct as the
        // resulting pointers, so it outlives them.
        let (reset, take_log, set_next_string, set_next_int, push_int) = unsafe {
            (
                *lib.get(b"MOCK_reset\0").expect("MOCK_reset"),
                *lib.get(b"MOCK_take_log\0").expect("MOCK_take_log"),
                *lib.get(b"MOCK_set_next_string\0").expect("set_next_string"),
                *lib.get(b"MOCK_set_next_int\0").expect("set_next_int"),
                *lib.get(b"MOCK_push_int\0").expect("push_int"),
            )
        };

        let reset: unsafe extern "system" fn() = reset;
        // SAFETY: no arguments, resolved from the library just opened.
        unsafe { reset() };

        let ai = AutoIt::builder()
            .dll_path(&path)
            .build()
            .expect("AutoIt should load against the mock");

        // Drop the AU3_Init that loading emitted, so tests see only their own
        // calls.
        // SAFETY: as above.
        unsafe { reset() };

        Self {
            guard,
            ai,
            lib,
            take_log,
            set_next_string,
            set_next_int,
            push_int,
        }
    }

    /// The recorded calls, one `NAME(arg, ...)` line each.
    ///
    /// `AU3_error` is filtered out. The safe layer reads it after every call —
    /// that is the whole point of the `au3!` macro — but it is an
    /// implementation detail, not something a test should have to interleave
    /// into its expectations.
    pub fn log(&self) -> String {
        self.calls().join("\n")
    }

    /// The recorded calls as a list, excluding the error-flag reads.
    pub fn calls(&self) -> Vec<String> {
        // SAFETY: a null buffer is the documented "how much do I need?" query.
        let needed = unsafe { (self.take_log)(std::ptr::null_mut(), 0) };
        let mut buf = vec![0u8; needed.max(1) as usize];
        // SAFETY: `buf` is writable for exactly `needed` bytes.
        unsafe { (self.take_log)(buf.as_mut_ptr(), needed) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());

        String::from_utf8_lossy(&buf[..end])
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with("AU3_error("))
            .map(str::to_owned)
            .collect()
    }

    /// Just the function names of the recorded calls.
    pub fn call_names(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| c.split('(').next().unwrap_or_default().to_owned())
            .collect()
    }

    /// Scripts what the next output-string call writes back.
    pub fn script_string(&self, s: &str) {
        let w: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `w` is NUL-terminated and outlives the call.
        unsafe { (self.set_next_string)(w.as_ptr()) };
    }

    /// Scripts what integer-returning calls return once the queue is empty.
    pub fn script_int(&self, v: i32) {
        // SAFETY: a plain integer, resolved from the loaded mock.
        unsafe { (self.set_next_int)(v) };
    }

    /// Queues integer return values, consumed in order.
    ///
    /// For flows whose answer changes between calls — an escalating close,
    /// where the window exists, ignores the request, and only goes away after
    /// the process is killed.
    pub fn script_ints(&self, values: &[i32]) {
        for v in values {
            // SAFETY: a plain integer, resolved from the loaded mock.
            unsafe { (self.push_int)(*v) };
        }
    }
}
