//! Failures that can happen before a single AU3 call is made.

use std::path::PathBuf;

/// Why the AutoItX3 DLL could not be loaded.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No DLL was found in any searched location.
    ///
    /// The message lists every path tried, in order. This is by far the most
    /// common first-run failure, so it is worth the verbosity: a bare "not
    /// found" sends people hunting, while the list usually makes the fix
    /// obvious.
    #[error(
        "AutoItX3 DLL not found. Searched, in order:\n{}\n\
         Set AUTOITX_DLL to the file, or AUTOITX_DIR to its directory. \
         See https://docs.rs/autoitx-sys for the full search order.",
        format_searched(.searched)
    )]
    NotFound {
        /// Every candidate path tried, in search order.
        searched: Vec<PathBuf>,
    },

    /// A DLL was found at `path` but could not be opened.
    ///
    /// Usually a bitness mismatch — `AutoItX3.dll` (32-bit) in a 64-bit
    /// process, or the x64 DLL in an ARM64 process.
    #[error("failed to open {path}: {source}")]
    Open {
        /// The file that failed to load.
        path: PathBuf,
        /// The underlying loader error.
        #[source]
        source: libloading::Error,
    },

    /// The library opened, but is missing an expected export.
    ///
    /// In practice this means the file is not an AutoItX3 DLL at all, or is
    /// dramatically older than the 3.3.14.x this crate targets.
    #[error(
        "the library loaded but does not export {name} — \
         is this really an AutoItX3 DLL? ({source})"
    )]
    MissingSymbol {
        /// The symbol that could not be resolved.
        name: &'static str,
        /// The underlying loader error.
        #[source]
        source: libloading::Error,
    },

    /// This build cannot host the DLL at all.
    ///
    /// `AutoItX3_x64.dll` is x86-64. It cannot load into an ARM64 process, and
    /// there is no ARM64 build of it. Target `x86_64-pc-windows-msvc` and run
    /// under emulation, or wait for the native backend.
    #[error("{reason}")]
    UnsupportedTarget {
        /// A specific explanation, rather than a generic load failure.
        reason: &'static str,
    },
}

fn format_searched(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "  (nowhere — no candidate paths were produced)".to_owned();
    }
    paths
        .iter()
        .map(|p| format!("  ✗ {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found_lists_every_path_tried() {
        let err = LoadError::NotFound {
            searched: vec![PathBuf::from("/a/AutoItX3_x64.dll"), PathBuf::from("/b")],
        };
        let msg = err.to_string();
        assert!(msg.contains("/a/AutoItX3_x64.dll"), "{msg}");
        assert!(msg.contains("/b"), "{msg}");
        // The remedy has to be in the message; users read errors, not docs.
        assert!(msg.contains("AUTOITX_DLL"), "{msg}");
    }

    #[test]
    fn not_found_with_no_candidates_still_reads_sensibly() {
        let msg = LoadError::NotFound { searched: vec![] }.to_string();
        assert!(msg.contains("nowhere"), "{msg}");
    }
}
