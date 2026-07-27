//! Reading the colour of what is on screen.
//!
//! # This is the one thing that needs Screen Recording
//!
//! Everything else in this backend gets by with the Accessibility grant.
//! Capturing pixels is the exception, and deliberately so: it means a robot
//! that only drives windows and keystrokes never has to ask for the permission
//! that lets a process read the whole screen.
//!
//! Without the grant, `CGDisplayCreateImageForRect` does not fail — it returns
//! a picture of the desktop wallpaper with every window missing. That is far
//! worse than an error, so the permission is checked up front.
//!
//! # Points and pixels
//!
//! The capture API takes a rectangle in **points** and hands back an image in
//! **physical pixels** — twice as many in each direction on a Retina display.
//! [`Capture`] keeps both and converts, so callers stay in the logical
//! coordinate space the rest of the crate uses.

#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

use super::permissions::{self, Permission};
use crate::error::{Error, Result};
use crate::{Point, Rect};
use objc2_core_foundation::{CFData, CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGDataProvider, CGImage, CGImageAlphaInfo, CGMainDisplayID};

/// A captured region of the screen, and enough geometry to address it.
pub(crate) struct Capture {
    /// The pixel bytes, as the window server laid them out.
    data: CFRetained<CFData>,
    /// The requested region, in logical points.
    area: Rect,
    /// Bytes per row of the buffer — not necessarily `width * depth`, because
    /// the window server pads rows for alignment.
    stride: usize,
    /// Bytes per pixel.
    depth: usize,
    /// Image width and height, in physical pixels.
    pixel_w: usize,
    pixel_h: usize,
    /// Whether the byte order is BGRA rather than RGBA.
    bgra: bool,
}

impl Capture {
    /// Captures a region of the main display.
    ///
    /// # Errors
    ///
    /// [`Error::PermissionDenied`] without Screen Recording, and
    /// [`Error::Platform`] if the capture itself fails — an empty rectangle, a
    /// region entirely off-screen, or a display that has gone away.
    pub(crate) fn of(area: Rect) -> Result<Self> {
        if area.w <= 0 || area.h <= 0 {
            return Err(Error::Platform {
                operation: "capture an empty screen region",
                platform: "macOS",
            });
        }
        permissions::require(Permission::ScreenRecording)?;

        let rect = CGRect::new(
            CGPoint {
                x: f64::from(area.x),
                y: f64::from(area.y),
            },
            CGSize {
                width: f64::from(area.w),
                height: f64::from(area.h),
            },
        );

        // Deprecated in favour of ScreenCaptureKit, which is asynchronous,
        // wants a run loop, and cannot answer "what colour is this pixel"
        // without standing up a frame stream. For a synchronous point query
        // this is still the right call; when Apple removes it the replacement
        // is a whole capture session, not a different function.
        #[allow(
            deprecated,
            reason = "ScreenCaptureKit has no synchronous single-shot equivalent"
        )]
        let image = objc2_core_graphics::CGDisplayCreateImageForRect(CGMainDisplayID(), rect)
            .ok_or(Error::Platform {
                operation: "capture the screen",
                platform: "macOS",
            })?;

        Self::from_image(&image, area)
    }

    fn from_image(image: &CGImage, area: Rect) -> Result<Self> {
        let unreadable = || Error::Platform {
            operation: "read back the captured pixels",
            platform: "macOS",
        };

        let provider = CGImage::data_provider(Some(image)).ok_or_else(unreadable)?;
        let data = CGDataProvider::data(Some(&provider)).ok_or_else(unreadable)?;

        let bits = CGImage::bits_per_pixel(Some(image));
        if bits % 8 != 0 || bits < 24 {
            return Err(Error::Platform {
                operation: "capture the screen in a usable pixel format",
                platform: "macOS",
            });
        }

        // The window server hands back BGRA on every Mac shipped so far, but
        // the flag is read rather than assumed — getting this backwards swaps
        // red and blue, which is the kind of bug that survives review because
        // all the numbers still look plausible.
        let bgra = matches!(
            CGImage::alpha_info(Some(image)),
            CGImageAlphaInfo::First
                | CGImageAlphaInfo::NoneSkipFirst
                | CGImageAlphaInfo::PremultipliedFirst
        );

        Ok(Self {
            data,
            area,
            stride: CGImage::bytes_per_row(Some(image)),
            depth: bits / 8,
            pixel_w: CGImage::width(Some(image)),
            pixel_h: CGImage::height(Some(image)),
            bgra,
        })
    }

    /// Physical pixels per logical point.
    ///
    /// Derived from what came back rather than asked for, so a display mode
    /// this code has not seen still lands on the right pixel.
    fn scale(&self) -> usize {
        (self.pixel_w / (self.area.w.max(1) as usize)).max(1)
    }

    /// The colour at a screen point, as `0xRRGGBB`.
    ///
    /// `None` if the point is outside the captured region.
    pub(crate) fn color_at(&self, p: Point) -> Option<u32> {
        if p.x < self.area.x
            || p.y < self.area.y
            || p.x >= self.area.x + self.area.w
            || p.y >= self.area.y + self.area.h
        {
            return None;
        }
        let scale = self.scale();
        let px = (p.x - self.area.x) as usize * scale;
        let py = (p.y - self.area.y) as usize * scale;
        self.color_at_pixel(px, py)
    }

    /// The colour at a physical pixel within the capture.
    fn color_at_pixel(&self, px: usize, py: usize) -> Option<u32> {
        if px >= self.pixel_w || py >= self.pixel_h {
            return None;
        }

        let offset = py * self.stride + px * self.depth;
        let bytes = self.bytes();
        let pixel = bytes.get(offset..offset + self.depth)?;

        // Little-endian BGRA reads out of memory as B, G, R, A.
        let (r, g, b) = if self.bgra {
            (pixel[2], pixel[1], pixel[0])
        } else {
            (pixel[0], pixel[1], pixel[2])
        };
        Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    }

    fn bytes(&self) -> &[u8] {
        let len = self.data.length().max(0) as usize;
        let ptr = self.data.byte_ptr();
        if ptr.is_null() || len == 0 {
            return &[];
        }
        // SAFETY: CFData guarantees `byte_ptr` addresses `length` readable
        // bytes, and the borrow is tied to `&self`, which owns the CFData.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    /// The first point whose colour is within `variation` of `colour`, scanning
    /// left to right then top to bottom.
    ///
    /// `step` skips points the way AutoIt's does: a step of 2 samples every
    /// other point in each direction, which is four times faster and still
    /// finds anything wider than a point.
    pub(crate) fn search(&self, colour: u32, variation: u32, step: u32) -> Option<Point> {
        let step = step.max(1) as i32;
        let mut y = self.area.y;
        while y < self.area.y + self.area.h {
            let mut x = self.area.x;
            while x < self.area.x + self.area.w {
                let p = Point::new(x, y);
                if self
                    .color_at(p)
                    .is_some_and(|c| within(c, colour, variation))
                {
                    return Some(p);
                }
                x += step;
            }
            y += step;
        }
        None
    }

    /// A checksum over the region, for detecting that something changed.
    ///
    /// The number is meaningless on its own and is **not** comparable with what
    /// Windows produces for the same region: AutoIt does not document its
    /// algorithm, and on a Retina display the two backends do not even sample
    /// the same pixel grid. What holds on both platforms is the only property
    /// callers use — an unchanged screen gives the same number, a changed one
    /// gives a different number.
    pub(crate) fn checksum(&self, step: u32) -> u32 {
        let step = step.max(1) as i32;
        // FNV-1a: no dependency, well spread, stable across runs.
        let mut hash: u32 = 0x811c_9dc5;
        let mut y = self.area.y;
        while y < self.area.y + self.area.h {
            let mut x = self.area.x;
            while x < self.area.x + self.area.w {
                let colour = self.color_at(Point::new(x, y)).unwrap_or(0);
                for byte in colour.to_le_bytes() {
                    hash ^= u32::from(byte);
                    hash = hash.wrapping_mul(0x0100_0193);
                }
                x += step;
            }
            y += step;
        }
        hash
    }
}

/// Whether two colours are within `variation` on every channel.
///
/// AutoIt's shade-variation rule, which is per-channel rather than a distance
/// in colour space.
fn within(a: u32, b: u32, variation: u32) -> bool {
    let channel = |c: u32, shift: u32| (c >> shift) & 0xFF;
    [16u32, 8, 0]
        .iter()
        .all(|&s| channel(a, s).abs_diff(channel(b, s)) <= variation)
}

/// The colour of a single screen point, as `0xRRGGBB`.
pub(crate) fn color_at(p: Point) -> Result<u32> {
    // A 1×1 capture is the whole request. There is no cheaper API for one
    // pixel, and caching a larger capture would hand stale colours to the poll
    // loops this exists to serve.
    let capture = Capture::of(Rect::new(p.x, p.y, 1, 1))?;
    capture.color_at(p).ok_or(Error::Platform {
        operation: "read a pixel that lies outside every display",
        platform: "macOS",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_variation_is_per_channel_not_a_distance() {
        assert!(within(0x40_80_C0, 0x40_80_C0, 0));
        // Off by five on every channel, within a tolerance of five.
        assert!(within(0x40_80_C0, 0x45_85_C5, 5));
        // One channel outside the tolerance is a miss even though the other two
        // are exact — a Euclidean distance would call this a match.
        assert!(!within(0x40_80_C0, 0x40_80_D0, 5));
    }

    #[test]
    fn variation_is_symmetric() {
        assert_eq!(
            within(0x10_10_10, 0x20_20_20, 16),
            within(0x20_20_20, 0x10_10_10, 16)
        );
    }

    #[test]
    fn an_empty_region_is_refused_before_any_permission_is_asked_for() {
        // Checked first on purpose: this is a caller bug, and reporting it as a
        // missing permission would send someone to System Settings for nothing.
        let result = Capture::of(Rect::new(0, 0, 0, 0));
        assert!(matches!(result, Err(Error::Platform { .. })));
    }
}
