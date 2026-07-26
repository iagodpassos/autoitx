//! Coordinates, in one space, on every platform.
//!
//! All geometry in this crate is **top-left origin, primary display, logical
//! points**, with `i32` components.
//!
//! That choice needs defending on macOS, where three coordinate systems meet:
//!
//! - **CGEvent, `CGWindowListCopyWindowInfo`, and Accessibility `AXPosition`**
//!   are already top-left origin — the same as Win32. This crate stays here.
//! - **AppKit** (`NSScreen`, `NSWindow`) is bottom-left origin. Conversion
//!   happens only at the AppKit boundary, e.g. reading `visibleFrame` to
//!   maximize a window.
//! - **Points vs. pixels**: Accessibility and CGEvent speak points, but
//!   `CGDisplayCreateImageForRect` returns pixels — 2× on Retina. Pixel
//!   functions therefore take an explicit coordinate space; everything else is
//!   points.
//!
//! The practical consequence: a coordinate captured on a Retina Mac is not the
//! same number as on a Windows box. Prefer selectors and window-relative
//! offsets over absolute screen coordinates.

/// A point in logical, top-left-origin coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Point {
    /// Horizontal offset, increasing rightward.
    pub x: i32,
    /// Vertical offset, increasing downward.
    pub y: i32,
}

impl Point {
    /// The origin of the primary display.
    pub const ORIGIN: Self = Self { x: 0, y: 0 };

    /// Builds a point.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Translates by `dx`/`dy`, saturating rather than overflowing.
    #[must_use]
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
        }
    }
}

/// A width/height pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Size {
    /// Width in logical points.
    pub w: i32,
    /// Height in logical points.
    pub h: i32,
}

impl Size {
    /// Builds a size.
    #[must_use]
    pub const fn new(w: i32, h: i32) -> Self {
        Self { w, h }
    }
}

/// A rectangle as **x/y/width/height** — AutoIt's documented `WinGetPos` shape,
/// not the Win32 `RECT` left/top/right/bottom shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Rect {
    /// X coordinate of the left edge.
    pub x: i32,
    /// Y coordinate of the top edge.
    pub y: i32,
    /// Width in logical points.
    pub w: i32,
    /// Height in logical points.
    pub h: i32,
}

impl Rect {
    /// Builds a rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// The top-left corner.
    #[must_use]
    pub const fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The width/height.
    #[must_use]
    pub const fn size(self) -> Size {
        Size::new(self.w, self.h)
    }

    /// Resolves a window-relative offset to an absolute screen point.
    ///
    /// This is the operation behind `recipes::click_in_window`, and the reason
    /// it exists: clicking at a fixed offset inside a window survives the window
    /// moving, where a hard-coded screen coordinate does not.
    #[must_use]
    pub const fn point_at(self, dx: i32, dy: i32) -> Point {
        self.origin().offset(dx, dy)
    }

    /// Whether `p` falls inside this rectangle, right/bottom edges exclusive.
    #[must_use]
    pub const fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }
}

impl From<autoitx_sys::RECT> for Rect {
    /// Converts a Win32 `RECT` (left/top/right/bottom) to x/y/w/h.
    fn from(r: autoitx_sys::RECT) -> Self {
        Self {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        }
    }
}

/// Which coordinate space a pixel operation is expressed in.
///
/// Only relevant on Retina-class displays, where a logical point covers more
/// than one physical pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum PixelCoordSpace {
    /// Logical points — consistent with the rest of this crate. The default.
    #[default]
    Points,
    /// Physical pixels — what the underlying capture API returns.
    Pixels,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_win32_converts_edges_to_extents() {
        let r = autoitx_sys::RECT {
            left: 100,
            top: 50,
            right: 400,
            bottom: 250,
        };
        assert_eq!(Rect::from(r), Rect::new(100, 50, 300, 200));
    }

    #[test]
    fn point_at_resolves_window_relative_offsets() {
        // A production RPA clicks "OK" at window-relative (600, 420).
        let window = Rect::new(160, 90, 1280, 720);
        assert_eq!(window.point_at(600, 420), Point::new(760, 510));
    }

    #[test]
    fn contains_excludes_right_and_bottom_edges() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(Point::new(0, 0)));
        assert!(r.contains(Point::new(9, 9)));
        assert!(!r.contains(Point::new(10, 9)));
        assert!(!r.contains(Point::new(9, 10)));
    }

    #[test]
    fn offset_saturates_instead_of_overflowing() {
        assert_eq!(Point::new(i32::MAX, 0).offset(1, 0).x, i32::MAX);
        assert_eq!(Point::new(i32::MIN, 0).offset(-1, 0).x, i32::MIN);
    }
}
