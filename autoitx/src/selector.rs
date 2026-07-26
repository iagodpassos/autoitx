//! Naming a window.
//!
//! AutoIt identifies windows two different ways, and the difference is easy to
//! miss because both are "just a string":
//!
//! ```text
//! "Order Entry"                        <- a bare title
//! "[CLASS:Chrome_WidgetWin_1;TITLE:Acme]" <- advanced syntax
//! ```
//!
//! A **bare title** is matched according to the current
//! [`TitleMatchMode`](crate::options::TitleMatchMode), which defaults to
//! *starts-with*. Automation written against that default silently depends on
//! it — `Selector::title("Order Entry")` also matches
//! `"Order Entry - Filter"`. This type keeps the two forms distinct so
//! the behaviour is visible rather than implied.
//!
//! ```
//! use autoitx::Selector;
//!
//! let by_title = Selector::title("Order Entry");
//! let advanced: Selector = "[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]".parse()?;
//! let active = Selector::active();
//!
//! let built = Selector::builder()
//!     .class("ui60Modal_W32")
//!     .title("Acme ERP")
//!     .build()?;
//! # Ok::<(), autoitx::selector::SelectorError>(())
//! ```
//!
//! # Portability
//!
//! Selectors are the least portable part of this crate, and pretending
//! otherwise would not help anyone. `CLASS:Chrome_WidgetWin_1` is a Win32
//! window class and means nothing on macOS, where the equivalent is a bundle
//! identifier. Use [`SelectorSet`] to keep one table of both rather than
//! spreading `#[cfg]` through business logic.

use std::fmt;
use std::str::FromStr;

/// One matching condition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Criterion {
    /// Window title, matched per the current title-match mode.
    Title(String),
    /// Win32 window class. On macOS, matched against the bundle identifier and
    /// then the accessibility role.
    Class(String),
    /// Title matched as a regular expression.
    RegexpTitle(String),
    /// Class matched as a regular expression.
    RegexpClass(String),
    /// Left edge, in screen coordinates.
    X(i32),
    /// Top edge, in screen coordinates.
    Y(i32),
    /// Width.
    W(i32),
    /// Height.
    H(i32),
    /// The nth match, 1-based, when several windows match.
    Instance(u32),
    /// The most recently active matching window.
    Last,
    /// The currently active window.
    Active,
    /// All matching windows.
    All,
    /// A specific window handle.
    Handle(u64),
    /// Owning process id.
    Pid(u32),
}

impl Criterion {
    /// The property name AutoIt uses for this criterion.
    const fn key(&self) -> &'static str {
        match self {
            Self::Title(_) => "TITLE",
            Self::Class(_) => "CLASS",
            Self::RegexpTitle(_) => "REGEXPTITLE",
            Self::RegexpClass(_) => "REGEXPCLASS",
            Self::X(_) => "X",
            Self::Y(_) => "Y",
            Self::W(_) => "W",
            Self::H(_) => "H",
            Self::Instance(_) => "INSTANCE",
            Self::Last => "LAST",
            Self::Active => "ACTIVE",
            Self::All => "ALL",
            Self::Handle(_) => "HANDLE",
            Self::Pid(_) => "PID",
        }
    }
}

impl fmt::Display for Criterion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = self.key();
        match self {
            Self::Last | Self::Active | Self::All => f.write_str(key),
            Self::Title(v) | Self::Class(v) | Self::RegexpTitle(v) | Self::RegexpClass(v) => {
                write!(f, "{key}:{v}")
            }
            Self::X(v) | Self::Y(v) | Self::W(v) | Self::H(v) => write!(f, "{key}:{v}"),
            Self::Instance(v) => write!(f, "{key}:{v}"),
            Self::Pid(v) => write!(f, "{key}:{v}"),
            // AutoIt expects handles in hex, without the 0x prefix.
            Self::Handle(v) => write!(f, "{key}:{v:x}"),
        }
    }
}

/// How a [`Selector`] identifies its window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Kind {
    /// A bare title string, subject to the current title-match mode.
    BareTitle(String),
    /// Advanced `[PROP:value;...]` syntax.
    Advanced(Vec<Criterion>),
}

/// A window selector.
///
/// See the [module docs](self) for the difference between the two forms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Selector(Kind);

impl Selector {
    /// A bare title, matched per the current title-match mode (prefix, by
    /// default).
    ///
    /// This is the form the .NET code uses for ERP dialogs — `"Order Entry de
    /// Pedidos"`, `"Confirm Delete"` — and it relies on prefix matching.
    #[must_use]
    pub fn title(t: impl Into<String>) -> Self {
        Self(Kind::BareTitle(t.into()))
    }

    /// The currently active window: `[ACTIVE]`.
    #[must_use]
    pub fn active() -> Self {
        Self(Kind::Advanced(vec![Criterion::Active]))
    }

    /// A specific window handle.
    #[must_use]
    pub fn handle(h: u64) -> Self {
        Self(Kind::Advanced(vec![Criterion::Handle(h)]))
    }

    /// Starts building an advanced selector.
    #[must_use]
    pub fn builder() -> SelectorBuilder {
        SelectorBuilder::default()
    }

    /// The criteria, or `None` for a bare title.
    #[must_use]
    pub fn criteria(&self) -> Option<&[Criterion]> {
        match &self.0 {
            Kind::Advanced(c) => Some(c),
            Kind::BareTitle(_) => None,
        }
    }

    /// The bare title, or `None` for an advanced selector.
    #[must_use]
    pub fn bare_title(&self) -> Option<&str> {
        match &self.0 {
            Kind::BareTitle(t) => Some(t),
            Kind::Advanced(_) => None,
        }
    }

    /// Whether this selector means "whatever window is focused".
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(&self.0, Kind::Advanced(c) if c.contains(&Criterion::Active))
    }
}

impl fmt::Display for Selector {
    /// Renders the form AutoIt expects.
    ///
    /// Round-trips: `Selector::from_str(&s.to_string()) == s` for every
    /// selector this crate can construct.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Kind::BareTitle(t) => f.write_str(t),
            Kind::Advanced(criteria) => {
                f.write_str("[")?;
                for (i, c) in criteria.iter().enumerate() {
                    if i > 0 {
                        f.write_str(";")?;
                    }
                    write!(f, "{c}")?;
                }
                f.write_str("]")
            }
        }
    }
}

impl From<&str> for Selector {
    /// Parses advanced syntax, or treats the string as a bare title.
    ///
    /// Infallible so that `"Order Entry".into()` works. Use
    /// [`FromStr`] when a malformed `[...]` should be an error rather than a
    /// title that happens to start with a bracket.
    fn from(s: &str) -> Self {
        s.parse().unwrap_or_else(|_| Self::title(s))
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Why a selector could not be built or parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SelectorError {
    /// A property name AutoIt does not recognise.
    #[error("unknown selector property {name:?}")]
    UnknownProperty {
        /// The property as written.
        name: String,
    },

    /// A property that needs a value did not get one, or vice versa.
    #[error("property {name} {problem}")]
    BadValue {
        /// The property name.
        name: String,
        /// What was wrong with its value.
        problem: String,
    },

    /// A value contains a character AutoIt's syntax cannot express.
    ///
    /// `;` separates criteria and `]` closes the selector, and AutoIt provides
    /// no escape for either. Emitting one anyway produces a selector that
    /// silently matches the wrong window — so this is an error rather than a
    /// best effort. The .NET code has no such check.
    #[error(
        "selector value {value:?} contains {ch:?}, which AutoIt cannot escape \
         inside a selector — match on a different property instead"
    )]
    UnescapableChar {
        /// The offending value.
        value: String,
        /// The character that cannot appear.
        ch: char,
    },

    /// Advanced syntax with nothing in it.
    #[error("selector has no criteria")]
    Empty,

    /// The string is not advanced syntax at all.
    #[error("not advanced selector syntax: expected a leading '[' and trailing ']'")]
    NotAdvanced,
}

/// Rejects values AutoIt's selector syntax cannot represent.
fn check_value(value: &str) -> Result<(), SelectorError> {
    for ch in [';', ']'] {
        if value.contains(ch) {
            return Err(SelectorError::UnescapableChar {
                value: value.to_owned(),
                ch,
            });
        }
    }
    Ok(())
}

impl FromStr for Selector {
    type Err = SelectorError;

    /// Parses advanced `[PROP:value;...]` syntax.
    ///
    /// Property names are matched case-insensitively, as AutoIt does — the
    /// .NET code mixes `[ACTIVE]` and `[active]`, and both must work.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = s
            .strip_prefix('[')
            .and_then(|r| r.strip_suffix(']'))
            .ok_or(SelectorError::NotAdvanced)?;

        if inner.trim().is_empty() {
            return Err(SelectorError::Empty);
        }

        let mut criteria = Vec::new();
        for part in inner.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (name, value) = match part.split_once(':') {
                Some((n, v)) => (n.trim(), Some(v)),
                None => (part, None),
            };
            criteria.push(parse_criterion(name, value)?);
        }

        if criteria.is_empty() {
            return Err(SelectorError::Empty);
        }
        Ok(Self(Kind::Advanced(criteria)))
    }
}

fn parse_criterion(name: &str, value: Option<&str>) -> Result<Criterion, SelectorError> {
    let upper = name.to_ascii_uppercase();

    let need_value = || -> Result<&str, SelectorError> {
        value.ok_or_else(|| SelectorError::BadValue {
            name: upper.clone(),
            problem: "requires a value".to_owned(),
        })
    };

    let need_int = |v: &str| -> Result<i32, SelectorError> {
        v.trim()
            .parse::<i32>()
            .map_err(|_| SelectorError::BadValue {
                name: upper.clone(),
                problem: format!("expected an integer, got {v:?}"),
            })
    };

    Ok(match upper.as_str() {
        "TITLE" => Criterion::Title(need_value()?.to_owned()),
        "CLASS" => Criterion::Class(need_value()?.to_owned()),
        "REGEXPTITLE" => Criterion::RegexpTitle(need_value()?.to_owned()),
        "REGEXPCLASS" => Criterion::RegexpClass(need_value()?.to_owned()),
        "X" => Criterion::X(need_int(need_value()?)?),
        "Y" => Criterion::Y(need_int(need_value()?)?),
        "W" => Criterion::W(need_int(need_value()?)?),
        "H" => Criterion::H(need_int(need_value()?)?),
        "INSTANCE" => {
            let v = need_value()?;
            Criterion::Instance(v.trim().parse().map_err(|_| SelectorError::BadValue {
                name: upper.clone(),
                problem: format!("expected a positive integer, got {v:?}"),
            })?)
        }
        "PID" => {
            let v = need_value()?;
            Criterion::Pid(v.trim().parse().map_err(|_| SelectorError::BadValue {
                name: upper.clone(),
                problem: format!("expected a process id, got {v:?}"),
            })?)
        }
        "HANDLE" => {
            let v = need_value()?.trim();
            let hex = v
                .strip_prefix("0x")
                .or_else(|| v.strip_prefix("0X"))
                .unwrap_or(v);
            Criterion::Handle(u64::from_str_radix(hex, 16).map_err(|_| {
                SelectorError::BadValue {
                    name: upper.clone(),
                    problem: format!("expected a hex handle, got {v:?}"),
                }
            })?)
        }
        "LAST" => Criterion::Last,
        "ACTIVE" => Criterion::Active,
        "ALL" => Criterion::All,
        _ => {
            return Err(SelectorError::UnknownProperty {
                name: name.to_owned(),
            });
        }
    })
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builds an advanced selector, rejecting values AutoIt cannot express.
#[derive(Debug, Default, Clone)]
pub struct SelectorBuilder {
    criteria: Vec<Criterion>,
    error: Option<SelectorError>,
}

macro_rules! string_criterion {
    ($(#[$m:meta])* $method:ident => $variant:ident) => {
        $(#[$m])*
        #[must_use]
        pub fn $method(mut self, v: impl Into<String>) -> Self {
            let v = v.into();
            match check_value(&v) {
                Ok(()) => self.criteria.push(Criterion::$variant(v)),
                // Keep the first error and let the chain continue, so `build()`
                // reports it rather than the caller having to `?` every step.
                Err(e) => {
                    self.error.get_or_insert(e);
                }
            }
            self
        }
    };
}

impl SelectorBuilder {
    string_criterion!(
        /// Matches the window title.
        title => Title
    );
    string_criterion!(
        /// Matches the Win32 window class.
        class => Class
    );
    string_criterion!(
        /// Matches the title against a regular expression.
        regexp_title => RegexpTitle
    );
    string_criterion!(
        /// Matches the class against a regular expression.
        regexp_class => RegexpClass
    );

    /// Selects the nth match, 1-based.
    #[must_use]
    pub fn instance(mut self, n: u32) -> Self {
        self.criteria.push(Criterion::Instance(n));
        self
    }

    /// Restricts to a process id.
    #[must_use]
    pub fn pid(mut self, pid: u32) -> Self {
        self.criteria.push(Criterion::Pid(pid));
        self
    }

    /// Adds an arbitrary criterion.
    #[must_use]
    pub fn criterion(mut self, c: Criterion) -> Self {
        self.criteria.push(c);
        self
    }

    /// Finishes the selector.
    ///
    /// # Errors
    ///
    /// [`SelectorError::UnescapableChar`] if any value contained `;` or `]`,
    /// or [`SelectorError::Empty`] if nothing was added.
    pub fn build(self) -> Result<Selector, SelectorError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.criteria.is_empty() {
            return Err(SelectorError::Empty);
        }
        Ok(Selector(Kind::Advanced(self.criteria)))
    }
}

// ---------------------------------------------------------------------------
// Cross-platform selector tables
// ---------------------------------------------------------------------------

/// One logical window, named per platform.
///
/// `[CLASS:Chrome_WidgetWin_1]` identifies Chrome on Windows; on macOS the same
/// window is `[CLASS:com.google.Chrome]`. Keeping both here means selector
/// differences stay in a table instead of leaking `#[cfg]` into automation
/// logic.
///
/// ```
/// use autoitx::selector::SelectorSet;
/// use autoitx::Selector;
///
/// let chrome = SelectorSet::new(
///     Selector::from("[CLASS:Chrome_WidgetWin_1]"),
///     Selector::from("[CLASS:com.google.Chrome]"),
/// );
/// let sel = chrome.current();   // whichever suits this build
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorSet {
    windows: Selector,
    macos: Selector,
}

impl SelectorSet {
    /// Pairs a Windows selector with its macOS equivalent.
    #[must_use]
    pub const fn new(windows: Selector, macos: Selector) -> Self {
        Self { windows, macos }
    }

    /// The selector for the platform this was compiled for.
    #[must_use]
    pub const fn current(&self) -> &Selector {
        #[cfg(windows)]
        {
            &self.windows
        }
        #[cfg(not(windows))]
        {
            &self.macos
        }
    }

    /// The Windows selector, whatever the current platform.
    #[must_use]
    pub const fn windows(&self) -> &Selector {
        &self.windows
    }

    /// The macOS selector, whatever the current platform.
    #[must_use]
    pub const fn macos(&self) -> &Selector {
        &self.macos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every selector the production RPAs use.
    const PRODUCTION_SHAPES: &[&str] = &[
        "[CLASS:Chrome_WidgetWin_1]",
        "[CLASS:Chrome_WidgetWin_1;TITLE:Acme ERP]",
        "[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]",
        "[TITLE:Acme ERP;CLASS:ui60Modal_W32]",
        "[REGEXPTITLE:Acme - (.*)Invoice(.*)]",
        "[REGEXPTITLE:Acme - (.*)Picking(.*)]",
        "[REGEXPTITLE:Acme - NORTHWIND(.*)Receiving(.*)]",
        "[REGEXPTITLE:Cancel Invoiced Items(.*)]",
        "[REGEXPTITLE:DevTools - (.*)]",
        "[ACTIVE]",
    ];

    #[test]
    fn every_real_selector_parses_and_round_trips() {
        for s in PRODUCTION_SHAPES {
            let sel: Selector = s.parse().unwrap_or_else(|e| panic!("{s:?}: {e}"));
            assert_eq!(&sel.to_string(), s, "round trip changed {s:?}");
            assert_eq!(
                sel.to_string().parse::<Selector>().unwrap(),
                sel,
                "reparse differs for {s:?}"
            );
        }
    }

    #[test]
    fn property_names_are_case_insensitive() {
        // The .NET code writes both `[ACTIVE]` and `[active]`.
        let upper: Selector = "[ACTIVE]".parse().unwrap();
        let lower: Selector = "[active]".parse().unwrap();
        assert_eq!(upper, lower);
        assert!(lower.is_active());

        let mixed: Selector = "[Class:Foo;title:Bar]".parse().unwrap();
        assert_eq!(mixed.to_string(), "[CLASS:Foo;TITLE:Bar]");
    }

    #[test]
    fn a_bare_title_stays_a_bare_title() {
        // Not advanced syntax: this must not become `[TITLE:...]`, because the
        // two are matched differently.
        let s = Selector::from("Order Entry");
        assert_eq!(s.bare_title(), Some("Order Entry"));
        assert_eq!(s.criteria(), None);
        assert_eq!(s.to_string(), "Order Entry");
    }

    #[test]
    fn criterion_order_is_preserved() {
        // AutoIt does not care, but a changed order makes diffs and logs
        // confusing, and breaks the round-trip property.
        let a: Selector = "[CLASS:X;TITLE:Y]".parse().unwrap();
        let b: Selector = "[TITLE:Y;CLASS:X]".parse().unwrap();
        assert_ne!(a.to_string(), b.to_string());
        assert_eq!(a.to_string(), "[CLASS:X;TITLE:Y]");
    }

    #[test]
    fn unescapable_characters_are_rejected_rather_than_emitted() {
        // The .NET code would happily build a broken selector here.
        let err = Selector::builder().title("A;B").build().unwrap_err();
        assert!(matches!(
            err,
            SelectorError::UnescapableChar { ch: ';', .. }
        ));

        let err = Selector::builder().class("A]B").build().unwrap_err();
        assert!(matches!(
            err,
            SelectorError::UnescapableChar { ch: ']', .. }
        ));
    }

    #[test]
    fn malformed_advanced_syntax_is_rejected() {
        for s in ["[NOPE:1]", "[]", "[TITLE]", "[X:abc]", "[INSTANCE:-1]"] {
            assert!(s.parse::<Selector>().is_err(), "should not parse: {s:?}");
        }
    }

    #[test]
    fn from_str_falls_back_to_a_title_for_non_bracket_strings() {
        // `From<&str>` is infallible so `"...".into()` works; a string that is
        // not advanced syntax becomes a bare title.
        assert_eq!(Selector::from("Forms").bare_title(), Some("Forms"));
        // But a malformed bracket string also becomes a title rather than
        // silently matching nothing.
        assert_eq!(Selector::from("[NOPE:1]").bare_title(), Some("[NOPE:1]"));
        // While `FromStr` reports the error.
        assert!("[NOPE:1]".parse::<Selector>().is_err());
    }

    #[test]
    fn handles_render_as_hex_without_a_prefix() {
        let s = Selector::handle(0x0004_0B1E);
        assert_eq!(s.to_string(), "[HANDLE:40b1e]");
        assert_eq!(s.to_string().parse::<Selector>().unwrap(), s);
        // Both `0x`-prefixed and bare hex parse.
        assert_eq!("[HANDLE:0x40b1e]".parse::<Selector>().unwrap(), s);
    }

    #[test]
    fn builder_produces_the_same_string_as_the_parser() {
        let built = Selector::builder()
            .class("Chrome_WidgetWin_1")
            .title("Acme Invoices")
            .build()
            .unwrap();
        let parsed: Selector = "[CLASS:Chrome_WidgetWin_1;TITLE:Acme Invoices]"
            .parse()
            .unwrap();
        assert_eq!(built, parsed);
    }

    #[test]
    fn selector_set_picks_by_platform() {
        let set = SelectorSet::new(
            Selector::from("[CLASS:Chrome_WidgetWin_1]"),
            Selector::from("[CLASS:com.google.Chrome]"),
        );
        if cfg!(windows) {
            assert_eq!(set.current(), set.windows());
        } else {
            assert_eq!(set.current(), set.macos());
        }
    }
}
