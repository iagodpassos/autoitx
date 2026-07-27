//! Platform backends.
//!
//! Today there is one: [`dll`], which calls into the AutoItX3 DLL. It is gated
//! on `windows` or the `mock-loader` feature, because those are the two places
//! a compatible library exists — the real DLL, and this project's test mock.
//!
//! The macOS backend (Accessibility, CGEvent, NSPasteboard) lands in a later
//! phase. A shared trait will be introduced then, when there are two
//! implementations to abstract over — one implementation behind a trait is
//! speculation, not design.

#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub(crate) mod dll;

#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub(crate) mod win32;

#[cfg(any(target_os = "macos", docsrs))]
pub(crate) mod macos;
