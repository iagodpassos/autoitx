//! A safe surface over `AXUIElement`.
//!
//! The Accessibility API is a stringly-typed attribute store: you ask an
//! element for `"AXTitle"` and get back a `CFType` you have to recognise. This
//! module does the recognising once, so the rest of the backend can ask for a
//! `String` or a `CGPoint` and get one.
//!
//! # Attribute names
//!
//! Apple ships these as `kAX*Attribute` constants, which the objc2 bindings do
//! not re-export — they are ordinary strings, so they are spelled out here. The
//! values are stable API, not implementation detail.

#![allow(
    dead_code,
    reason = "unused when the mock-loader feature selects the DLL backend instead"
)]

use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGPoint, CGSize};
use std::ptr::NonNull;

// Attributes. Apple's `kAX*Attribute` constants, verbatim.
pub(crate) const ATTR_WINDOWS: &str = "AXWindows";
pub(crate) const ATTR_TITLE: &str = "AXTitle";
pub(crate) const ATTR_POSITION: &str = "AXPosition";
pub(crate) const ATTR_SIZE: &str = "AXSize";
pub(crate) const ATTR_ROLE: &str = "AXRole";
pub(crate) const ATTR_SUBROLE: &str = "AXSubrole";
pub(crate) const ATTR_FOCUSED_WINDOW: &str = "AXFocusedWindow";
pub(crate) const ATTR_FOCUSED: &str = "AXFocused";
pub(crate) const ATTR_MAIN: &str = "AXMain";
pub(crate) const ATTR_MINIMIZED: &str = "AXMinimized";
pub(crate) const ATTR_CHILDREN: &str = "AXChildren";
pub(crate) const ATTR_VALUE: &str = "AXValue";
pub(crate) const ATTR_FRONTMOST: &str = "AXFrontmost";

// Actions.
pub(crate) const ACTION_PRESS: &str = "AXPress";
pub(crate) const ACTION_RAISE: &str = "AXRaise";

/// The accessibility element for an application, by process id.
pub(crate) fn app_element(pid: i32) -> CFRetained<AXUIElement> {
    // SAFETY: takes a pid and returns a retained element; no preconditions
    // beyond the process existing, and a dead pid yields an element whose
    // every query fails rather than something unsound.
    unsafe { AXUIElement::new_application(pid) }
}

/// Reads an attribute, or `None` when it is absent or unreadable.
///
/// Absence is deliberately not an error. Half the attributes here are optional
/// — a window may have no subrole, an element no title — and threading a
/// `Result` through that would drown the real failures.
pub(crate) fn attribute(element: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let key = CFString::from_str(name);
    let mut raw: *const CFType = std::ptr::null();

    // SAFETY: `raw` is a valid out-pointer for the duration of the call, and
    // the API writes a retained pointer into it on success.
    let err = unsafe {
        element.copy_attribute_value(&key, NonNull::from(&mut raw).cast::<*const CFType>())
    };

    if err != AXError::Success || raw.is_null() {
        return None;
    }
    // SAFETY: the API returned Success with a non-null pointer, which by its
    // contract is a +1 reference we now own.
    Some(unsafe { CFRetained::from_raw(NonNull::new(raw.cast_mut())?) })
}

/// Reads a string attribute.
pub(crate) fn string_attribute(element: &AXUIElement, name: &str) -> Option<String> {
    let value = attribute(element, name)?;
    value.downcast_ref::<CFString>().map(|s| s.to_string())
}

/// Reads a boolean attribute.
pub(crate) fn bool_attribute(element: &AXUIElement, name: &str) -> Option<bool> {
    let value = attribute(element, name)?;
    value
        .downcast_ref::<objc2_core_foundation::CFBoolean>()
        .map(|b| b.as_bool())
}

/// Reads an array attribute as its elements.
pub(crate) fn element_array(element: &AXUIElement, name: &str) -> Vec<CFRetained<AXUIElement>> {
    let Some(value) = attribute(element, name) else {
        return Vec::new();
    };
    let Some(array) = value.downcast_ref::<CFArray>() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for i in 0..array.count() {
        // SAFETY: `i` is within the array's own reported count.
        let raw = unsafe { array.value_at_index(i) };
        if raw.is_null() {
            continue;
        }
        // SAFETY: CFArray's values are borrowed, so retain before keeping one.
        let item = unsafe {
            CFRetained::retain(
                NonNull::new(raw.cast_mut().cast::<AXUIElement>()).expect("checked non-null above"),
            )
        };
        out.push(item);
    }
    out
}

/// Reads a `CGPoint`-shaped attribute, such as `AXPosition`.
pub(crate) fn point_attribute(element: &AXUIElement, name: &str) -> Option<CGPoint> {
    let value = attribute(element, name)?;
    let ax = value.downcast_ref::<AXValue>()?;
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    // SAFETY: the out-pointer matches the type being requested.
    let ok = unsafe {
        ax.value(
            AXValueType::CGPoint,
            NonNull::from(&mut point).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some(point)
}

/// Reads a `CGSize`-shaped attribute, such as `AXSize`.
pub(crate) fn size_attribute(element: &AXUIElement, name: &str) -> Option<CGSize> {
    let value = attribute(element, name)?;
    let ax = value.downcast_ref::<AXValue>()?;
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    // SAFETY: the out-pointer matches the type being requested.
    let ok = unsafe {
        ax.value(
            AXValueType::CGSize,
            NonNull::from(&mut size).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some(size)
}

/// Writes a `CGPoint`-shaped attribute.
pub(crate) fn set_point(element: &AXUIElement, name: &str, mut p: CGPoint) -> bool {
    // SAFETY: the value pointer matches the declared type.
    let Some(value) = (unsafe {
        AXValue::new(
            AXValueType::CGPoint,
            NonNull::from(&mut p).cast::<std::ffi::c_void>(),
        )
    }) else {
        return false;
    };
    let key = CFString::from_str(name);
    // SAFETY: setting an attribute to a value of the type it expects.
    unsafe { element.set_attribute_value(&key, &value) == AXError::Success }
}

/// Writes a `CGSize`-shaped attribute.
pub(crate) fn set_size(element: &AXUIElement, name: &str, mut s: CGSize) -> bool {
    // SAFETY: the value pointer matches the declared type.
    let Some(value) = (unsafe {
        AXValue::new(
            AXValueType::CGSize,
            NonNull::from(&mut s).cast::<std::ffi::c_void>(),
        )
    }) else {
        return false;
    };
    let key = CFString::from_str(name);
    // SAFETY: setting an attribute to a value of the type it expects.
    unsafe { element.set_attribute_value(&key, &value) == AXError::Success }
}

/// Writes a boolean attribute.
pub(crate) fn set_bool(element: &AXUIElement, name: &str, v: bool) -> bool {
    let key = CFString::from_str(name);
    let value = objc2_core_foundation::CFBoolean::new(v);
    // SAFETY: setting an attribute to a CFBoolean, which is what these expect.
    unsafe { element.set_attribute_value(&key, value) == AXError::Success }
}

/// Performs an action, such as pressing a button.
pub(crate) fn perform(element: &AXUIElement, action: &str) -> bool {
    let key = CFString::from_str(action);
    // SAFETY: performing a named action; unknown names fail rather than misbehave.
    unsafe { element.perform_action(&key) == AXError::Success }
}

/// Bounds how long a query to an unresponsive application may block.
///
/// The default is effectively forever, which is the wrong answer for
/// automation: a beachballing application would hang the robot rather than be
/// reported as busy. Also the mechanism behind
/// [`is_app_responsive`](super::window::is_app_responsive).
pub(crate) fn set_timeout(element: &AXUIElement, seconds: f32) {
    // SAFETY: sets a per-element timeout; no ownership or lifetime effects.
    unsafe {
        let _ = element.set_messaging_timeout(seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dead_process_yields_an_element_whose_queries_fail_quietly() {
        // Rather than panicking or returning something unsound. Automation
        // routinely races a process that just exited, so this path is normal.
        let element = app_element(999_999);
        assert!(string_attribute(&element, ATTR_TITLE).is_none());
        assert!(element_array(&element, ATTR_WINDOWS).is_empty());
        assert!(point_attribute(&element, ATTR_POSITION).is_none());
    }

    #[test]
    fn an_unknown_attribute_is_absent_rather_than_an_error() {
        let element = app_element(std::process::id() as i32);
        assert!(attribute(&element, "AXDefinitelyNotAnAttribute").is_none());
    }
}
