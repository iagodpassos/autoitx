//! The `AU3_*` C ABI, bound to a loaded DLL.
//!
//! An internal `au3_api!` macro consumes the declaration list from
//! [`crate::functions`] and generates three things that must never drift apart:
//!
//! 1. [`Au3`] — a struct of function pointers, one per export.
//! 2. `Au3::bind` — the loader that resolves every symbol.
//! 3. [`Au3::SYMBOLS`] — the symbol names, which a test compares against the
//!    real DLL's export table (`tests/data/au3_exports.txt`) in both
//!    directions.
//!
//! Declaring a function is therefore the only step: forgetting to load it, or
//! typo'ing its name, cannot compile-and-pass.
//!
//! # Conventions
//!
//! Everything is `extern "system"`. On x86_64, `WINAPI`/`__stdcall` is a no-op
//! alias for the single Microsoft x64 calling convention, so these declarations
//! serve both `-msvc` and `-gnu`.
//!
//! (AutoIt's own header declares `AU3_error` without `WINAPI`, unlike every
//! other export. On x64 that is a distinction without a difference; it would
//! matter on x86, which this crate does not support.)
//!
//! Optional integer parameters take [`AU3_INTDEFAULT`] to mean "use the AutoIt
//! default". Optional string parameters take a pointer to an empty string, not
//! null — AutoItX does not null-check.
//!
//! [`AU3_INTDEFAULT`]: crate::types::AU3_INTDEFAULT

use crate::types::{DWORD, HWND, PCWSTR, POINT, PWSTR, RECT};

/// Generates [`Au3`], its loader, and its symbol table from a function list.
///
/// See the [module docs](self) for why this is a macro rather than 117
/// hand-written declarations.
macro_rules! au3_api {
    ($(
        $(#[$meta:meta])*
        fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? ;
    )+) => {
        /// Every `AU3_*` entry point, resolved from a loaded AutoItX3 DLL.
        ///
        /// # Safety
        ///
        /// The function pointers are only valid while `_lib` is alive. Copying
        /// one out of this struct and calling it after the `Au3` has been
        /// dropped is undefined behaviour — the DLL will have been unloaded.
        /// Keep the `Au3` alive for as long as any call may happen; the safe
        /// `autoitx` crate does this by holding it in an `Arc` for the process
        /// lifetime.
        #[allow(non_snake_case)]
        #[non_exhaustive]
        pub struct Au3 {
            /// Kept alive so the function pointers stay valid. Dropping this
            /// calls `FreeLibrary`.
            _lib: ::libloading::Library,
            $(
                // A generated line first, so every field satisfies
                // `deny(missing_docs)` without 117 boilerplate comments; any
                // hand-written docs below append to it.
                #[doc = concat!("Binding to `", stringify!($name), "`.")]
                $(#[$meta])*
                pub $name: unsafe extern "system" fn($($ty),*) $(-> $ret)?,
            )+
        }

        impl Au3 {
            /// Every symbol name this crate binds, in declaration order.
            ///
            /// A test asserts this matches the real DLL's export table exactly,
            /// so a missing or misspelled binding fails CI rather than
            /// producing a confusing runtime `MissingSymbol`.
            pub const SYMBOLS: &'static [&'static str] = &[$(stringify!($name)),+];

            /// Resolves every symbol in `lib`.
            ///
            /// # Safety
            ///
            /// `lib` must be an AutoItX3 DLL (or an ABI-compatible stand-in
            /// such as the test mock). Binding these signatures to a library
            /// that exports the same names with different types is undefined
            /// behaviour.
            // The locals below are named after the symbols they hold, so that
            // the struct can be built with field-init shorthand.
            #[allow(non_snake_case)]
            // Only reachable through `load_from`, which carries the same gate.
            #[cfg(any(windows, feature = "mock-loader", docsrs))]
            unsafe fn bind(lib: ::libloading::Library) -> Result<Self, crate::LoadError> {
                $(
                    // SAFETY: the caller guarantees `lib` is ABI-compatible.
                    // Dereferencing the `Symbol` erases its borrow of `lib`,
                    // which is sound because `lib` is moved into the same
                    // struct as the resulting pointer and outlives it.
                    let $name = unsafe {
                        let sym: ::libloading::Symbol<
                            unsafe extern "system" fn($($ty),*) $(-> $ret)?,
                        > = lib
                            .get(concat!(stringify!($name), "\0").as_bytes())
                            .map_err(|source| crate::LoadError::MissingSymbol {
                                name: stringify!($name),
                                source,
                            })?;
                        *sym
                    };
                )+
                Ok(Self { _lib: lib, $($name),+ })
            }
        }
    };
}

// The list itself lives in `functions.rs` so the mock DLL can generate itself
// from the very same declarations. See that module for why.
crate::au3_functions!(au3_api);

impl Au3 {
    /// Opens `path` as an AutoItX3 DLL, resolves every symbol, and calls
    /// `AU3_Init`.
    ///
    /// `AU3_Init` is what establishes AutoIt's default option table (absolute
    /// mouse coordinates, prefix window-title matching, a 5 ms key delay). It
    /// is called exactly once here, so callers never have to remember to.
    ///
    /// # Safety
    ///
    /// `path` must name an AutoItX3 DLL or an ABI-compatible stand-in. Loading
    /// an unrelated library that happens to export these names is undefined
    /// behaviour. Loading also runs the library's initialisation code, which is
    /// arbitrary.
    ///
    /// # Availability
    ///
    /// Off Windows this requires the `mock-loader` feature. There is no real
    /// AutoItX DLL for other platforms, so the only thing to load is a
    /// stand-in — and making that explicit stops "why does `autoitx-sys` build
    /// on my Mac but find nothing?" from being a question.
    #[cfg(any(windows, feature = "mock-loader", docsrs))]
    #[cfg_attr(docsrs, doc(cfg(any(windows, feature = "mock-loader"))))]
    pub unsafe fn load_from(path: &std::path::Path) -> Result<Self, crate::LoadError> {
        // SAFETY: delegated to this function's own contract.
        let lib = unsafe { ::libloading::Library::new(path) }.map_err(|source| {
            crate::LoadError::Open {
                path: path.to_path_buf(),
                source,
            }
        })?;

        // SAFETY: same contract — `lib` is asserted ABI-compatible by caller.
        let au3 = unsafe { Self::bind(lib) }?;

        // SAFETY: resolved from the library just loaded; takes no arguments.
        unsafe { (au3.AU3_Init)() };

        Ok(au3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_count_matches_the_real_dll() {
        // AutoItX 3.3.14.5 exports 117 AU3_* functions. See
        // tests/data/au3_exports.txt for the frozen table and its provenance.
        assert_eq!(Au3::SYMBOLS.len(), 117);
    }

    #[test]
    fn every_symbol_is_au3_prefixed_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for name in Au3::SYMBOLS {
            assert!(name.starts_with("AU3_"), "{name} lacks the AU3_ prefix");
            assert!(seen.insert(*name), "{name} declared twice");
        }
    }
}
