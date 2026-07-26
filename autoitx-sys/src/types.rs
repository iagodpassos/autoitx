//! The complete Win32 surface the AutoItX ABI needs.
//!
//! Deliberately hand-rolled instead of pulling in `windows-sys`: every AU3
//! parameter is a pointer or an `int`, and `LPRECT`/`LPPOINT` are always
//! *out*-pointers, never passed by value. Four definitions cover all 117
//! functions — and because there is no `windows-sys` dependency, this crate
//! compiles cleanly on macOS, which is what makes the mock-DLL test strategy
//! (and cross-checking from a Mac) possible.

use core::ffi::c_void;

/// A borrowed, NUL-terminated UTF-16 string. Win32's `LPCWSTR`.
pub type PCWSTR = *const u16;

/// A caller-allocated UTF-16 output buffer. Win32's `LPWSTR`.
///
/// Always paired with an `i32` capacity counted in wide characters
/// **including the terminating NUL**.
pub type PWSTR = *mut u16;

/// Win32's `DWORD`.
pub type DWORD = u32;

/// An opaque window handle. `HWND` on Windows.
///
/// Kept as a raw pointer to match the C ABI exactly. The safe `autoitx` crate
/// converts this to a plain `u64` before it reaches public API, so that window
/// handles stay `Copy + Send + Sync` and portable.
pub type HWND = *mut c_void;

/// Win32 `RECT`: **left/top/right/bottom**, not x/y/width/height.
///
/// AutoIt's documented `WinGetPos` semantics are x/y/w/h, but the C ABI hands
/// back a real `RECT`. The conversion happens in the safe layer; this type must
/// not leak into public API.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RECT {
    /// X coordinate of the left edge.
    pub left: i32,
    /// Y coordinate of the top edge.
    pub top: i32,
    /// X coordinate of the right edge (exclusive).
    pub right: i32,
    /// Y coordinate of the bottom edge (exclusive).
    pub bottom: i32,
}

/// Win32 `POINT`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct POINT {
    /// X coordinate.
    pub x: i32,
    /// Y coordinate.
    pub y: i32,
}

/// Sentinel meaning "use the AutoIt default" for optional integer parameters.
///
/// Note this is `i32::MIN + 1`, **not** `i32::MIN` — an easy off-by-one to get
/// wrong, which is why it is defined exactly once here and never typed as a
/// literal anywhere else in the workspace.
pub const AU3_INTDEFAULT: i32 = -2_147_483_647;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intdefault_is_min_plus_one() {
        assert_eq!(AU3_INTDEFAULT, i32::MIN + 1);
        assert_ne!(AU3_INTDEFAULT, i32::MIN);
    }

    #[test]
    fn win32_structs_have_c_layout() {
        assert_eq!(size_of::<RECT>(), 16);
        assert_eq!(size_of::<POINT>(), 8);
        assert_eq!(align_of::<RECT>(), 4);
    }
}
