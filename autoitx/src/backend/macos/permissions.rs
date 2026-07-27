//! macOS privacy permissions, surfaced rather than hidden.
//!
//! Without the Accessibility grant, every AX call returns `kAXErrorAPIDisabled`
//! — which reads exactly like "window not found". Automation that does not
//! check this sends you hunting for a wrong selector for an hour. So every
//! AX-backed operation checks first, and the error says what to do.

use objc2_application_services::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};

/// A macOS privacy permission this crate may need.
///
/// Grants are keyed to a binary's path **and** code signature, which has a
/// consequence worth knowing before it wastes an afternoon: `cargo build`
/// rewrites the binary, and every `cargo test` run produces a fresh
/// hash-suffixed one, so macOS re-prompts constantly during development.
/// Granting the permission to your terminal or IDE (children inherit it), or
/// ad-hoc signing with `codesign -s - --force`, avoids the churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Permission {
    /// Required for every window and control operation.
    ///
    /// This is the one automation cannot work without.
    Accessibility,
    /// Required only for pixel and screen-capture operations.
    ///
    /// Deliberately not requested up front: window titles come from the
    /// Accessibility API rather than from `CGWindowListCopyWindowInfo`, so the
    /// common case never needs this.
    ScreenRecording,
}

/// Whether a [`Permission`] has been granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionStatus {
    /// Granted; the corresponding APIs will work.
    Granted,
    /// Denied, or never granted. macOS does not distinguish the two for
    /// Accessibility, so neither does this.
    Denied,
}

impl PermissionStatus {
    /// Whether the permission is usable.
    #[must_use]
    pub const fn is_granted(self) -> bool {
        matches!(self, Self::Granted)
    }
}

impl Permission {
    /// A `x-apple.systempreferences:` deep link to this permission's pane.
    ///
    /// Embedded in permission errors so the message is actionable rather than
    /// merely accurate.
    #[must_use]
    pub const fn settings_url(self) -> &'static str {
        match self {
            Self::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Self::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }

    /// What to tell the user, deep link included.
    #[must_use]
    pub fn hint(self) -> String {
        let what = match self {
            Self::Accessibility => "Accessibility",
            Self::ScreenRecording => "Screen Recording",
        };
        format!(
            "grant {what} to this program in System Settings > Privacy & Security > {what}, \
             then run it again. Open that pane with:\n  open \"{}\"\n\
             Note the grant is tied to the binary's path and signature, so a rebuilt \
             binary may need granting again.",
            self.settings_url()
        )
    }
}

/// Whether a permission is granted, without prompting.
///
/// Safe to call at any time, including in a tight loop: it does not show UI.
#[must_use]
pub fn check(permission: Permission) -> PermissionStatus {
    let granted = match permission {
        // SAFETY: takes no arguments and only reads TCC state.
        Permission::Accessibility => unsafe { AXIsProcessTrusted() },
        // The "preflight" form is documented not to prompt, and objc2 marks it
        // safe accordingly.
        Permission::ScreenRecording => CGPreflightScreenCaptureAccess(),
    };
    if granted {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

/// Asks for a permission, showing the system prompt if it has not been decided.
///
/// Returns the status *after* asking — which, for Accessibility, is almost
/// always still `Denied`: macOS shows a dialog pointing at System Settings and
/// the user has to go there and come back. Treat this as "the prompt was
/// shown", not as "the answer arrived".
pub fn request(permission: Permission) -> PermissionStatus {
    match permission {
        Permission::Accessibility => {
            // SAFETY: reading an `extern static` provided by the framework.
            let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
            let yes = CFBoolean::new(true);
            let options: CFRetained<CFDictionary<CFString, CFBoolean>> =
                CFDictionary::from_slices(&[key], &[yes]);

            // `as_opaque` drops the key/value types: the AX function takes an
            // untyped CFDictionary, and the safety obligation it states — that
            // the generics are right — is discharged by having built it from
            // the documented key and a CFBoolean one line above.
            // SAFETY: as described.
            let granted = unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
            if granted {
                PermissionStatus::Granted
            } else {
                PermissionStatus::Denied
            }
        }
        // Prompts on first call, then returns the stored answer.
        Permission::ScreenRecording => {
            if CGRequestScreenCaptureAccess() {
                PermissionStatus::Granted
            } else {
                PermissionStatus::Denied
            }
        }
    }
}

/// Fails with an actionable error unless `permission` is granted.
#[allow(
    dead_code,
    reason = "used by the AX-backed operations landing in phase 5"
)]
pub(crate) fn require(permission: Permission) -> crate::Result<()> {
    if check(permission).is_granted() {
        return Ok(());
    }
    Err(crate::Error::PermissionDenied {
        permission,
        hint: permission.hint(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_urls_are_distinct_and_well_formed() {
        let a = Permission::Accessibility.settings_url();
        let s = Permission::ScreenRecording.settings_url();
        assert_ne!(a, s);
        for url in [a, s] {
            assert!(url.starts_with("x-apple.systempreferences:"));
        }
    }

    #[test]
    fn the_hint_is_actionable_not_merely_accurate() {
        // A permission error that only says "denied" costs the reader a web
        // search. This one has to carry the command that fixes it.
        let h = Permission::Accessibility.hint();
        assert!(h.contains("System Settings"), "{h}");
        assert!(h.contains("open \"x-apple.systempreferences:"), "{h}");
        assert!(
            h.contains("rebuilt"),
            "the rebuild gotcha must be mentioned"
        );
    }

    #[test]
    fn checking_never_panics_whatever_the_grant_state() {
        // Runs in CI with no grant and locally with one; both must be fine, and
        // neither may show UI.
        let _ = check(Permission::Accessibility);
        let _ = check(Permission::ScreenRecording);
    }
}
