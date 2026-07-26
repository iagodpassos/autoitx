//! A fake `AutoItX3.dll`.
//!
//! Exports the `AU3_*` symbols with the real calling convention and signatures,
//! records what it was called with, and returns scripted values. Pointing
//! `AUTOITX_DLL` at the artifact lets the entire Windows marshalling layer —
//! UTF-16 round-tripping, output-buffer growth, the `AU3_error` protocol,
//! argument order across all 117 functions — be tested on macOS as an ordinary
//! `cargo test`.
//!
//! Not published. Filled in during phase 1, alongside the real signatures.

#![allow(clippy::missing_safety_doc)]

/// Placeholder export proving the `cdylib` builds and the ABI is declared the
/// way the loader expects. Replaced by the generated signature set in phase 1.
///
/// # Safety
///
/// Called across an FFI boundary; takes no arguments and touches no state.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn AU3_Init() {}
