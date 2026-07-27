//! Platform backends.
//!
//! Two, chosen by `cfg`, with the same method surface:
//!
//! - [`dll`] calls into the AutoItX3 DLL. Selected on Windows, and on any
//!   platform when the `mock-loader` feature is on — that is how the DLL
//!   marshalling gets tested from a Mac.
//! - [`macos`] is a native implementation over Apple's own frameworks.
//!
//! There is no shared trait. Both are `pub(crate)` and only one exists in any
//! given build, so a trait would buy dynamic dispatch nobody wants and hide
//! which one is in play.

#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub(crate) mod dll;

#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub(crate) mod win32;

#[cfg(target_os = "macos")]
pub(crate) mod macos;
