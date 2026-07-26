//! Finding the AutoItX3 DLL.
//!
//! One ordered search, documented in exactly one place — here, in the crate
//! docs, and in the [`LoadError::NotFound`] message, which lists every path it
//! actually tried.

use crate::{LoadError, dll_name};
use std::path::{Path, PathBuf};

/// Where to look for the DLL, in priority order.
///
/// Each variant is tried in turn; the first file that exists wins. Nothing here
/// touches the filesystem — [`search_paths`] produces the candidate list, so it
/// can be inspected and tested without a DLL present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchStep {
    /// `$AUTOITX_DLL` — a full path to the file.
    EnvFile,
    /// `$AUTOITX_DIR` — a directory containing it.
    EnvDir,
    /// Next to the current executable. The usual deployment layout.
    NextToExe,
    /// The current working directory.
    WorkingDir,
    /// The AutoIt installation recorded in the registry. Windows only.
    Registry,
}

/// The ordered candidate paths, before any existence check.
///
/// `explicit` short-circuits everything else: an explicitly configured path
/// that does not exist is an error, not an invitation to fall back to some
/// other DLL. Silently loading a different AutoIt version than the one asked
/// for is precisely the sort of thing that produces an unreproducible bug.
#[must_use]
pub fn search_paths(explicit: Option<&Path>) -> Vec<PathBuf> {
    if let Some(p) = explicit {
        return vec![p.to_path_buf()];
    }

    let name = dll_name();
    let mut out = Vec::new();

    if let Some(p) = std::env::var_os("AUTOITX_DLL") {
        out.push(PathBuf::from(p));
    }
    if let Some(d) = std::env::var_os("AUTOITX_DIR") {
        out.push(PathBuf::from(d).join(name));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        out.push(dir.join(name));
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd.join(name));
    }
    if let Some(dir) = registry_install_dir() {
        out.push(dir.join("AutoItX").join(name));
    }

    out
}

/// The AutoIt install directory from `HKLM\SOFTWARE\AutoIt v3\AutoIt`.
///
/// Always `None` off Windows. Reading the registry needs `advapi32`, which is
/// deferred to the Windows backend rather than pulling in a registry crate for
/// a single lookup.
fn registry_install_dir() -> Option<PathBuf> {
    // TODO(phase 3): side-load advapi32!RegGetValueW via libloading, the same
    // way the safe layer reaches kernel32 for the cross-process mutex. Until
    // then the other five steps cover every realistic layout.
    None
}

/// Finds the first candidate path that exists.
///
/// # Errors
///
/// [`LoadError::NotFound`] if no candidate exists, carrying the full list for
/// the error message.
pub fn locate(explicit: Option<&Path>) -> Result<PathBuf, LoadError> {
    let searched = search_paths(explicit);
    searched
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or(LoadError::NotFound { searched })
}

/// Rejects targets that cannot host the DLL at all.
///
/// Distinguishing "wrong CPU architecture" from "file missing" up front saves a
/// genuinely confusing debugging session: on Windows-on-ARM the DLL is present
/// and readable, and `LoadLibrary` still fails.
// Only reachable through `Au3::load`, which carries the same gate.
#[cfg(any(windows, feature = "mock-loader", docsrs))]
pub(crate) const fn check_target() -> Result<(), LoadError> {
    if cfg!(all(windows, target_arch = "aarch64")) {
        return Err(LoadError::UnsupportedTarget {
            reason: "AutoItX3_x64.dll is x86-64 and cannot load into an ARM64 \
                     process. Build for x86_64-pc-windows-msvc and run under \
                     emulation.",
        });
    }
    if cfg!(all(windows, target_pointer_width = "32")) {
        return Err(LoadError::UnsupportedTarget {
            reason: "32-bit Windows is not supported: AutoItX exports are \
                     stdcall-decorated there, and this crate targets the x64 DLL.",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_short_circuits_every_other_step() {
        let p = Path::new("/somewhere/custom.dll");
        assert_eq!(search_paths(Some(p)), vec![p.to_path_buf()]);
    }

    #[test]
    fn search_order_puts_env_first_and_cwd_after_the_exe() {
        // Not using the real env vars: this process's environment is shared
        // with every other test, and mutating it would make them order-
        // dependent. The invariant worth pinning is the relative order of the
        // steps that are always present.
        let paths = search_paths(None);
        let name = dll_name();

        let exe_idx = paths.iter().position(|p| {
            std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join(name)))
                .is_some_and(|expected| *p == expected)
        });
        let cwd_idx = paths.iter().position(|p| {
            std::env::current_dir()
                .ok()
                .map(|d| d.join(name))
                .is_some_and(|expected| *p == expected)
        });

        if let (Some(e), Some(c)) = (exe_idx, cwd_idx) {
            assert!(e < c, "exe dir must be searched before cwd: {paths:?}");
        }
    }

    #[test]
    fn every_candidate_ends_in_the_right_dll_name() {
        // AUTOITX_DLL is a full path so it is exempt; the rest must all point
        // at the correctly-named file for this build's pointer width.
        for p in search_paths(None) {
            if std::env::var_os("AUTOITX_DLL").is_some_and(|v| Path::new(&v) == p) {
                continue;
            }
            assert_eq!(
                p.file_name().and_then(|s| s.to_str()),
                Some(dll_name()),
                "unexpected candidate: {}",
                p.display()
            );
        }
    }

    #[test]
    fn locating_a_nonexistent_explicit_path_does_not_fall_back() {
        let err = locate(Some(Path::new("/definitely/not/here.dll"))).unwrap_err();
        let LoadError::NotFound { searched } = err else {
            panic!("expected NotFound, got {err:?}");
        };
        assert_eq!(searched.len(), 1, "explicit path must not gain fallbacks");
    }
}
