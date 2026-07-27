//! Naming a control inside a window.
//!
//! Controls are how automation stops guessing at pixels. A coordinate click
//! only survives if the screen geometry does — which is why automation built
//! that way tends to pin a screen resolution and refuse to start when it
//! changes. Addressing the control directly survives the window moving, the
//! display changing, and the layout shifting.
//!
//! ```
//! use autoitx::Control;
//!
//! let by_class = Control::class_nn("Edit1");        // ClassnameNN
//! let by_id    = Control::id(1001);                 // the dialog control id
//! let by_text  = Control::text("&OK");              // visible text
//! let nth_edit = Control::builder().class("Edit").instance(2).build()?;
//! # Ok::<(), autoitx::selector::SelectorError>(())
//! ```
//!
//! # Finding out what is there
//!
//! AutoIt ships a Window Info tool for this, and
//! [`AutoIt::win_get_class_list`](crate::AutoIt::win_get_class_list) plus
//! [`AutoIt::control_get_focus`](crate::AutoIt::control_get_focus) answer the
//! same questions from code — useful when the window only exists mid-flow.

use crate::selector::SelectorError;
use std::fmt;

/// How to find a control within its window.
///
/// Renders to the identifier string AutoIt expects. The variants mirror
/// AutoIt's own vocabulary rather than inventing one, so knowledge transfers
/// from existing AutoIt automation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Control(String);

impl Control {
    /// A `ClassnameNN` identifier, as AutoIt's Window Info tool reports:
    /// `"Edit1"`, `"Button3"`.
    #[must_use]
    pub fn class_nn(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The control's dialog id — stable across localisation, unlike text.
    #[must_use]
    pub fn id(id: i32) -> Self {
        Self(format!("[ID:{id}]"))
    }

    /// The control's visible text, e.g. `"&OK"`.
    ///
    /// Convenient, but the first thing to break when an application is
    /// translated. Prefer [`id`](Self::id) where you know it.
    #[must_use]
    pub fn text(t: impl Into<String>) -> Self {
        Self(format!("[TEXT:{}]", t.into()))
    }

    /// A specific window handle, from
    /// [`control_get_handle`](crate::AutoIt::control_get_handle).
    #[must_use]
    pub fn handle(h: u64) -> Self {
        Self(format!("[HANDLE:{h:x}]"))
    }

    /// Builds an identifier from several criteria.
    #[must_use]
    pub fn builder() -> ControlBuilder {
        ControlBuilder::default()
    }

    /// The identifier string, as AutoIt reads it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Control {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Control {
    /// Treats the string as a `ClassnameNN`, or passes advanced syntax through.
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Builds a multi-criteria control identifier.
#[derive(Debug, Default, Clone)]
pub struct ControlBuilder {
    parts: Vec<(&'static str, String)>,
    error: Option<SelectorError>,
}

macro_rules! criterion {
    ($(#[$m:meta])* $method:ident => $key:literal) => {
        $(#[$m])*
        #[must_use]
        pub fn $method(mut self, v: impl Into<String>) -> Self {
            let v = v.into();
            // Same unescapable characters as window selectors: AutoIt has no
            // way to quote `;` or `]` inside one, so emitting either produces
            // an identifier that quietly matches the wrong control.
            if let Some(ch) = [';', ']'].into_iter().find(|c| v.contains(*c)) {
                self.error.get_or_insert(SelectorError::UnescapableChar {
                    value: v.clone(),
                    ch,
                });
            } else {
                self.parts.push(($key, v));
            }
            self
        }
    };
}

impl ControlBuilder {
    criterion!(
        /// The control's window class, e.g. `"Edit"`.
        class => "CLASS"
    );
    criterion!(
        /// The control's visible text.
        text => "TEXT"
    );
    criterion!(
        /// The control's internal name, where the toolkit provides one.
        name => "NAME"
    );
    criterion!(
        /// The class matched as a regular expression.
        regexp_class => "REGEXPCLASS"
    );

    /// The nth match, 1-based.
    #[must_use]
    pub fn instance(mut self, n: u32) -> Self {
        self.parts.push(("INSTANCE", n.to_string()));
        self
    }

    /// The dialog control id.
    #[must_use]
    pub fn id(mut self, id: i32) -> Self {
        self.parts.push(("ID", id.to_string()));
        self
    }

    /// Finishes the identifier.
    ///
    /// # Errors
    ///
    /// [`SelectorError::UnescapableChar`] if a value contained `;` or `]`, or
    /// [`SelectorError::Empty`] if nothing was added.
    pub fn build(self) -> Result<Control, SelectorError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.parts.is_empty() {
            return Err(SelectorError::Empty);
        }
        let inner = self
            .parts
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";");
        Ok(Control(format!("[{inner}]")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_forms_render_as_autoit_expects() {
        assert_eq!(Control::class_nn("Edit1").as_str(), "Edit1");
        assert_eq!(Control::id(1001).as_str(), "[ID:1001]");
        assert_eq!(Control::text("&OK").as_str(), "[TEXT:&OK]");
        assert_eq!(Control::handle(0x4_0B1E).as_str(), "[HANDLE:40b1e]");
    }

    #[test]
    fn the_builder_joins_criteria_with_semicolons() {
        let c = Control::builder()
            .class("Edit")
            .instance(2)
            .build()
            .unwrap();
        assert_eq!(c.as_str(), "[CLASS:Edit;INSTANCE:2]");
    }

    #[test]
    fn unescapable_characters_are_rejected() {
        // Same rule as window selectors, and for the same reason: AutoIt
        // cannot quote these, so emitting one matches the wrong thing quietly.
        for bad in ["A;B", "A]B"] {
            assert!(
                Control::builder().text(bad).build().is_err(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn an_empty_builder_is_an_error_rather_than_an_empty_identifier() {
        assert!(Control::builder().build().is_err());
    }
}
