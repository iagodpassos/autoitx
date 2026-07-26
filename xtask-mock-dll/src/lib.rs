//! A fake `AutoItX3.dll`.
//!
//! Exports every `AU3_*` symbol with the real calling convention and
//! signatures, records what it was called with, and returns scripted values.
//! Pointing `AUTOITX_DLL` at the built artifact lets the whole marshalling
//! layer — UTF-16 round-tripping, output buffers, `AU3_error`, argument order
//! across all 117 functions — be tested on macOS as an ordinary `cargo test`.
//!
//! The exports are generated from [`autoitx_sys::au3_functions`], the same list
//! that generates the real bindings. That is the point: a signature fixed in
//! one place is fixed in both, and a passing mock test proves the *real*
//! declaration is callable rather than a copy of it that happens to agree.
//!
//! Built as a `cdylib`, so it is a `.dylib` on macOS and a `.dll` on Windows.
//! `libloading` opens either.
//!
//! Not published.

use autoitx_sys::{AU3_INTDEFAULT, DWORD, HWND, PCWSTR, POINT, PWSTR, RECT};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Recorded state
// ---------------------------------------------------------------------------

/// One recorded call, rendered as `NAME(arg, arg, ...)`.
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// What the next output-string function should write back.
static NEXT_STRING: Mutex<Option<Vec<u16>>> = Mutex::new(None);
/// What integer-returning functions should return once the queue is empty.
static NEXT_INT: Mutex<i32> = Mutex::new(0);
/// Scripted return values, consumed in order.
///
/// Needed to exercise sequences where the answer changes between calls — an
/// escalating close, for instance, where the window exists, ignores the close
/// request, and only goes away after the process is killed. A single fixed
/// value cannot express that.
static RETURN_QUEUE: Mutex<std::collections::VecDeque<i32>> =
    Mutex::new(std::collections::VecDeque::new());
/// What `AU3_error` reports. Kept separate from the queue on purpose — see
/// [`Ret`].
static NEXT_ERROR: Mutex<i32> = Mutex::new(0);

/// The next scripted integer: the queue if it has one, else the fixed value.
///
/// Written without a `let` chain: those need Rust 1.88, and this workspace's
/// MSRV is 1.85.
fn next_int() -> i32 {
    let queued = RETURN_QUEUE.lock().ok().and_then(|mut q| q.pop_front());
    match queued {
        Some(v) => v,
        None => NEXT_INT.lock().map(|g| *g).unwrap_or(0),
    }
}

fn record(call: String) {
    if let Ok(mut g) = CALLS.lock() {
        g.push(call);
    }
}

// ---------------------------------------------------------------------------
// Argument rendering
// ---------------------------------------------------------------------------

/// One captured argument, kept structured rather than pre-rendered.
///
/// Structure is what lets the log post-pass recognise an output buffer: in
/// AutoItX's ABI a `PWSTR` is *always* immediately followed by its capacity, so
/// the pair is detectable by walking the captured list. Doing it here, at
/// runtime, avoids asking `macro_rules!` to pattern-match on types — which it
/// cannot do without ambiguity.
enum ArgVal {
    Int(i32),
    Str(String),
    OutBuf(PWSTR),
    Handle(HWND),
    RectOut(bool),
    PointOut(bool),
}

/// Captures one FFI argument.
///
/// Implemented per concrete pointer type so the macro needs no type dispatch.
trait Arg {
    fn capture(&self) -> ArgVal;
}

impl Arg for i32 {
    fn capture(&self) -> ArgVal {
        ArgVal::Int(*self)
    }
}

impl Arg for PCWSTR {
    /// Decodes an input wide string — the assertion that actually matters, and
    /// the reason non-ASCII round-tripping is testable without Windows.
    fn capture(&self) -> ArgVal {
        if self.is_null() {
            return ArgVal::Str("\0null".to_owned());
        }
        // AutoItX's contract is that every LPCWSTR is NUL-terminated; the code
        // under test must uphold it, and violating it is exactly the bug this
        // mock exists to surface.
        let mut len = 0usize;
        // SAFETY: walking to the NUL terminator the ABI guarantees.
        while unsafe { *self.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` wide chars were just proven readable.
        let slice = unsafe { std::slice::from_raw_parts(*self, len) };
        ArgVal::Str(String::from_utf16_lossy(slice))
    }
}

impl Arg for PWSTR {
    fn capture(&self) -> ArgVal {
        ArgVal::OutBuf(*self)
    }
}

impl Arg for HWND {
    fn capture(&self) -> ArgVal {
        ArgVal::Handle(*self)
    }
}

impl Arg for *mut RECT {
    fn capture(&self) -> ArgVal {
        ArgVal::RectOut(self.is_null())
    }
}

impl Arg for *mut POINT {
    fn capture(&self) -> ArgVal {
        ArgVal::PointOut(self.is_null())
    }
}

/// Renders the captured arguments and services any output buffer among them.
///
/// The `(PWSTR, i32)` pair is AutoItX's universal output-string convention, so
/// recognising it here covers every such function with no per-function work.
fn finish(name: &str, args: &[ArgVal]) -> String {
    let mut parts = Vec::with_capacity(args.len());

    for (i, a) in args.iter().enumerate() {
        parts.push(match a {
            // The sentinel is named, so a test asserting a parameter was
            // defaulted need not know the magic number.
            ArgVal::Int(v) if *v == AU3_INTDEFAULT => "INTDEFAULT".to_owned(),
            ArgVal::Int(v) => v.to_string(),
            ArgVal::Str(s) if s == "\0null" => "null".to_owned(),
            ArgVal::Str(s) => format!("{s:?}"),
            ArgVal::Handle(h) if h.is_null() => "hwnd:null".to_owned(),
            ArgVal::Handle(h) => format!("hwnd:{:#x}", *h as usize),
            ArgVal::RectOut(true) => "rect:null".to_owned(),
            ArgVal::RectOut(false) => "rect:out".to_owned(),
            ArgVal::PointOut(true) => "point:null".to_owned(),
            ArgVal::PointOut(false) => "point:out".to_owned(),
            ArgVal::OutBuf(buf) => {
                if let Some(ArgVal::Int(size)) = args.get(i + 1) {
                    fill_out_buffer(*buf, *size);
                }
                if buf.is_null() { "null" } else { "out" }.to_owned()
            }
        });
    }

    format!("{name}({})", parts.join(", "))
}

// ---------------------------------------------------------------------------
// Return values
// ---------------------------------------------------------------------------

/// Supplies a value for each return type the ABI uses.
///
/// Takes the function name because one function must not behave like the
/// others: `AU3_error` is a status read that the safe layer performs after
/// *every* call. If it drew from the scripted queue like an operation does, it
/// would consume half the script and shift every subsequent answer onto the
/// wrong call.
trait Ret {
    fn mock(name: &str) -> Self;
}

impl Ret for i32 {
    fn mock(name: &str) -> Self {
        if name == "AU3_error" {
            return NEXT_ERROR.lock().map(|g| *g).unwrap_or(0);
        }
        next_int()
    }
}

impl Ret for DWORD {
    fn mock(name: &str) -> Self {
        let _ = name;
        next_int() as Self
    }
}

impl Ret for HWND {
    fn mock(name: &str) -> Self {
        let _ = name;
        std::ptr::null_mut()
    }
}

/// Writes the scripted string into an output buffer, honouring `buf_size`.
///
/// AutoItX writes at most `buf_size` wide chars **including** the NUL and never
/// reports how much room it needed — so the mock truncates the same way, which
/// is what makes the safe layer's grow-and-retry loop testable.
fn fill_out_buffer(buf: PWSTR, buf_size: i32) {
    if buf.is_null() || buf_size <= 0 {
        return;
    }
    let scripted = NEXT_STRING.lock().ok().and_then(|g| g.clone());
    let src = scripted.unwrap_or_default();
    let cap = buf_size as usize;
    let n = src.len().min(cap - 1);
    // SAFETY: the caller guarantees `buf` is writable for `buf_size` wide
    // chars; `n + 1 <= cap` by construction.
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
}

// ---------------------------------------------------------------------------
// The generated exports
// ---------------------------------------------------------------------------

/// Emits one `#[no_mangle] extern "system"` shim per declared AU3 function.
macro_rules! mock_impl {
    ($(
        $(#[$meta:meta])*
        fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? ;
    )+) => {
        $(
            #[doc = concat!("Mock of `", stringify!($name), "`.")]
            ///
            /// # Safety
            ///
            /// Called across FFI with the AutoItX ABI. Pointer arguments must
            /// satisfy the same contract the real DLL requires.
            // The exported symbol must match the DLL's name byte for byte, so
            // Rust's naming convention cannot apply here.
            #[allow(non_snake_case)]
            #[unsafe(no_mangle)]
            pub unsafe extern "system" fn $name($($arg: $ty),*) $(-> $ret)? {
                // `finish` both renders the log line and services any output
                // buffer it finds, so no per-function special-casing is needed.
                record(finish(
                    stringify!($name),
                    &[$(Arg::capture(&$arg)),*],
                ));

                $(<$ret as Ret>::mock(stringify!($name)))?
            }
        )+
    };
}

autoitx_sys::au3_functions!(mock_impl);

// ---------------------------------------------------------------------------
// Test-control surface (not part of the AutoItX ABI)
// ---------------------------------------------------------------------------

/// Clears the call log and any scripted return values.
///
/// # Safety
///
/// Called across FFI; takes no arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_reset() {
    if let Ok(mut g) = CALLS.lock() {
        g.clear();
    }
    if let Ok(mut g) = NEXT_STRING.lock() {
        *g = None;
    }
    if let Ok(mut g) = NEXT_INT.lock() {
        *g = 0;
    }
    if let Ok(mut q) = RETURN_QUEUE.lock() {
        q.clear();
    }
    if let Ok(mut g) = NEXT_ERROR.lock() {
        *g = 0;
    }
}

/// Scripts what `AU3_error` reports after the next call.
///
/// # Safety
///
/// Called across FFI; takes a plain integer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_set_next_error(v: i32) {
    if let Ok(mut g) = NEXT_ERROR.lock() {
        *g = v;
    }
}

/// Number of AU3 calls recorded since the last reset.
///
/// # Safety
///
/// Called across FFI; takes no arguments.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_call_count() -> i32 {
    CALLS.lock().map(|g| g.len() as i32).unwrap_or(-1)
}

/// Copies the call log into `buf` as NUL-terminated UTF-8, one call per line.
///
/// Returns the number of bytes that *would* be needed, so a caller can retry
/// with a larger buffer.
///
/// # Safety
///
/// `buf` must be writable for `buf_size` bytes, or null to query the size.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_take_log(buf: *mut u8, buf_size: i32) -> i32 {
    let joined = CALLS
        .lock()
        .map(|g| g.join("\n"))
        .unwrap_or_else(|_| String::new());
    let bytes = joined.as_bytes();
    let needed = bytes.len() + 1;

    if !buf.is_null() && buf_size > 0 {
        let n = bytes.len().min(buf_size as usize - 1);
        // SAFETY: caller guarantees `buf` is writable for `buf_size` bytes,
        // and `n + 1 <= buf_size`.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
            *buf.add(n) = 0;
        }
    }
    needed as i32
}

/// Scripts what the next output-string function writes back.
///
/// # Safety
///
/// `s` must be a NUL-terminated wide string, or null to clear.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_set_next_string(s: PCWSTR) {
    let value = if s.is_null() {
        None
    } else {
        let mut len = 0usize;
        // SAFETY: caller guarantees NUL termination.
        while unsafe { *s.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` wide chars were just proven readable.
        Some(unsafe { std::slice::from_raw_parts(s, len) }.to_vec())
    };
    if let Ok(mut g) = NEXT_STRING.lock() {
        *g = value;
    }
}

/// Scripts what integer-returning functions return once the queue is empty.
///
/// # Safety
///
/// Called across FFI; takes a plain integer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_set_next_int(v: i32) {
    if let Ok(mut g) = NEXT_INT.lock() {
        *g = v;
    }
}

/// Queues one integer return value, consumed before the fixed one.
///
/// Call repeatedly to script a sequence of differing answers.
///
/// # Safety
///
/// Called across FFI; takes a plain integer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn MOCK_push_int(v: i32) {
    if let Ok(mut q) = RETURN_QUEUE.lock() {
        q.push_back(v);
    }
}
