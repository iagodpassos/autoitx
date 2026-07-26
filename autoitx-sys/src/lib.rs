//! Raw FFI bindings to the AutoItX3 DLL.
//!
//! This crate is the unsafe, 1:1 mirror of the `AU3_*` C ABI exported by
//! `AutoItX3_x64.dll`. Most users want the safe [`autoitx`] crate instead.
//!
//! [`autoitx`]: https://docs.rs/autoitx
//!
//! # Runtime loading, not link-time
//!
//! The DLL is opened with [`libloading`] at runtime rather than declared with
//! `#[link]`. This has one consequence that shapes the whole project: there is
//! no link-time dependency on anything Windows, so
//!
//! ```text
//! cargo check --target x86_64-pc-windows-gnu
//! ```
//!
//! type-checks the entire Windows backend from macOS or Linux, with no MSVC
//! toolchain, no mingw, and no import library.
//!
//! # Calling convention
//!
//! Everything is `extern "system"`. On x86_64, `WINAPI`/`__stdcall` is a no-op
//! alias for the single Microsoft x64 calling convention, so the same
//! declarations serve `x86_64-pc-windows-msvc` and `x86_64-pc-windows-gnu`.
//! Symbols are undecorated on x64.
//!
//! 32-bit Windows (`i686-*`) is **not supported**: there the exports are
//! stdcall-decorated (`_AU3_Init@0`), and the DLL this crate targets is x64.
//!
//! # Strings
//!
//! Every string is UTF-16. Inputs are `*const u16` (`LPCWSTR`); outputs are a
//! caller-allocated `*mut u16` plus an `int nBufSize` **counted in wide
//! characters, including the terminating NUL**. AutoItX always NUL-terminates
//! and never signals how much room it actually needed, so the safe layer grows
//! the buffer and retries when a call fills it to the brim.
//!
//! # Legal
//!
//! AutoIt and AutoItX are products of AutoIt Consulting Ltd. This project is
//! not affiliated with, endorsed by, or sponsored by them, and the AutoItX3
//! DLL is **not** distributed with this crate. See `NOTICE`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/autoitx-sys/0.1.0")]

// Declares `au3_functions!`, which `api` consumes. Must come first: macros are
// only visible to modules declared after their definition.
#[macro_use]
pub mod functions;

pub mod api;
pub mod error;
pub mod loader;
pub mod types;

pub use api::Au3;
pub use error::LoadError;
pub use types::{AU3_INTDEFAULT, DWORD, HWND, PCWSTR, POINT, PWSTR, RECT};

impl Au3 {
    /// Finds and loads the AutoItX3 DLL, then calls `AU3_Init`.
    ///
    /// Pass `explicit` to name the file directly; otherwise the search order in
    /// [`loader::search_paths`] applies. An explicit path never falls back —
    /// silently loading a different AutoIt build than the one requested is how
    /// unreproducible bugs are made.
    ///
    /// # Errors
    ///
    /// [`LoadError::UnsupportedTarget`] on a platform that cannot host the DLL,
    /// [`LoadError::NotFound`] if no candidate exists (the message lists every
    /// path tried), [`LoadError::Open`] if the file is not loadable — usually a
    /// bitness mismatch — or [`LoadError::MissingSymbol`] if it loads but is
    /// not an AutoItX3 DLL.
    ///
    /// # Safety
    ///
    /// Loading a shared library runs its initialisation code. The file found
    /// must genuinely be an AutoItX3 DLL (or an ABI-compatible stand-in such as
    /// this project's test mock); binding these signatures to something else
    /// that happens to export the same names is undefined behaviour.
    ///
    /// # Availability
    ///
    /// Off Windows this requires the `mock-loader` feature — see
    /// [`Au3::load_from`].
    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(any(windows, feature = "mock-loader"))))]
    pub unsafe fn load(explicit: Option<&std::path::Path>) -> Result<Self, LoadError> {
        loader::check_target()?;
        let path = loader::locate(explicit)?;
        // SAFETY: delegated to this function's own contract.
        unsafe { Self::load_from(&path) }
    }
}

/// The DLL file name AutoIt ships for 64-bit processes.
pub const DLL_NAME_X64: &str = "AutoItX3_x64.dll";

/// The DLL file name AutoIt ships for 32-bit processes.
pub const DLL_NAME_X86: &str = "AutoItX3.dll";

/// The DLL file name matching this build's pointer width.
pub const fn dll_name() -> &'static str {
    if cfg!(target_pointer_width = "64") {
        DLL_NAME_X64
    } else {
        DLL_NAME_X86
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dll_name_matches_pointer_width() {
        // The host running these tests is 64-bit in every supported config.
        assert_eq!(dll_name(), DLL_NAME_X64);
    }
}
