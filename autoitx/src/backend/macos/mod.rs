//! The native macOS backend.
//!
//! No AutoIt here: this is an independent implementation of the same API
//! surface, over Apple's own frameworks. Accessibility for windows and
//! controls, Core Graphics for input and pixels, AppKit for the pasteboard.
//!
//! # Coordinates
//!
//! Everything speaks **logical points, top-left origin, primary display** —
//! the same space Win32 uses, which is what lets the portable core mean one
//! thing on both platforms. Three systems meet here and only one of them is
//! that:
//!
//! - **CGEvent, `CGWindowListCopyWindowInfo`, and AX `AXPosition`** are already
//!   top-left origin. This backend stays here.
//! - **AppKit** (`NSScreen`, `NSWindow`) is bottom-left origin. Converted only
//!   at the AppKit boundary — reading `visibleFrame` to maximise a window.
//! - **Points versus pixels**: AX and CGEvent speak points;
//!   `CGDisplayCreateImageForRect` returns pixels, 2× on Retina. Pixel
//!   operations therefore take an explicit coordinate space.

pub(crate) mod clipboard;
pub(crate) mod permissions;
