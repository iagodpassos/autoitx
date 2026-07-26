//! Keystroke sequences, in AutoIt's `Send` language.
//!
//! `Send` interprets six characters specially — `{`, `}`, `!`, `+`, `^`, `#` —
//! so a string built by interpolating data is a keystroke-injection bug waiting
//! to happen. In the .NET code this crate replaces, a password containing `{`
//! is enough:
//!
//! ```csharp
//! AutoItUtils.Send(WMS_PASSWORD + "{ENTER}");   // hope there's no '{' in there
//! ```
//!
//! So this module makes the safe thing the short thing. Four ways in, ordered
//! by how much you should reach for them:
//!
//! ```
//! use autoitx::{keys, Keys};
//!
//! // 1. Data. Escaped — cannot become a command, whatever is in it.
//! let k = Keys::text("R$ 1.234,56 {not a token}");
//!
//! // 2. A literal command. Validated at compile time.
//! let k = keys!("{CTRLDOWN}c{CTRLUP}");
//!
//! // 3. A command built at run time. Validated, returns Result.
//! let k = Keys::parse("{TAB 4}")?;
//!
//! // 4. The escape hatch. Named so it shows up in review.
//! let k = Keys::raw_unchecked("{WHATEVER}");
//! # Ok::<(), autoitx::keys::KeyParseError>(())
//! ```
//!
//! # Compile-time validation
//!
//! [`keys!`](macro@crate::keys) runs the same validator as [`Keys::parse`] in a `const` block, so
//! a malformed literal fails to build:
//!
//! ```compile_fail
//! # use autoitx::keys;
//! let bad = keys!("{CTRLDOWN");   // unclosed — does not compile
//! ```
//!
//! No proc macro is involved: the grammar is small enough to validate with a
//! `const fn` over bytes.

use std::borrow::Cow;

/// The characters `Send` treats as syntax rather than input.
pub const SPECIAL_CHARS: [char; 6] = ['{', '}', '!', '+', '^', '#'];

// ---------------------------------------------------------------------------
// The key vocabulary
// ---------------------------------------------------------------------------

/// Every name valid inside `{...}`.
///
/// Matched case-insensitively, as AutoIt does. Kept as a flat array so the
/// `const fn` validator can walk it at compile time.
const NAMED_KEYS: &[&str] = &[
    // Editing and navigation
    "SPACE",
    "ENTER",
    "ESCAPE",
    "ESC",
    "TAB",
    "BACKSPACE",
    "BS",
    "DELETE",
    "DEL",
    "INSERT",
    "INS",
    "UP",
    "DOWN",
    "LEFT",
    "RIGHT",
    "HOME",
    "END",
    "PGUP",
    "PGDN",
    // Function keys
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    // Locks and system keys
    "CAPSLOCK",
    "NUMLOCK",
    "SCROLLLOCK",
    "PRINTSCREEN",
    "BREAK",
    "PAUSE",
    "APPSKEY",
    "SLEEP",
    // Modifiers, as one-shot names
    "ALT",
    "CTRL",
    "SHIFT",
    "LALT",
    "RALT",
    "LCTRL",
    "RCTRL",
    "LSHIFT",
    "RSHIFT",
    "LWIN",
    "RWIN",
    // Modifiers, as explicit down/up pairs. These dominate real automation:
    // `{CTRLDOWN}c{CTRLUP}` rather than `^c`.
    "ALTDOWN",
    "ALTUP",
    "SHIFTDOWN",
    "SHIFTUP",
    "CTRLDOWN",
    "CTRLUP",
    "LWINDOWN",
    "LWINUP",
    "RWINDOWN",
    "RWINUP",
    // Numeric keypad
    "NUMPAD0",
    "NUMPAD1",
    "NUMPAD2",
    "NUMPAD3",
    "NUMPAD4",
    "NUMPAD5",
    "NUMPAD6",
    "NUMPAD7",
    "NUMPAD8",
    "NUMPAD9",
    "NUMPADMULT",
    "NUMPADADD",
    "NUMPADSUB",
    "NUMPADDIV",
    "NUMPADDOT",
    "NUMPADENTER",
    // Send a character by code: {ASC 065}
    "ASC",
    // Browser and media keys
    "BROWSER_BACK",
    "BROWSER_FORWARD",
    "BROWSER_REFRESH",
    "BROWSER_STOP",
    "BROWSER_SEARCH",
    "BROWSER_FAVORITES",
    "BROWSER_HOME",
    "VOLUME_MUTE",
    "VOLUME_DOWN",
    "VOLUME_UP",
    "MEDIA_NEXT",
    "MEDIA_PREV",
    "MEDIA_STOP",
    "MEDIA_PLAY_PAUSE",
    "LAUNCH_MAIL",
    "LAUNCH_MEDIA",
    "LAUNCH_APP1",
    "LAUNCH_APP2",
];

// ---------------------------------------------------------------------------
// const-compatible validation
// ---------------------------------------------------------------------------

// `eq_ignore_ascii_case`, which clippy suggests here, is not a `const fn` — and
// running in const context is the entire point of this helper: it is what lets
// `keys!` reject a bad literal at compile time.
#[allow(clippy::manual_ignore_case_cmp)]
const fn eq_ascii_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].to_ascii_uppercase() != b[i].to_ascii_uppercase() {
            return false;
        }
        i += 1;
    }
    true
}

const fn is_known_name(name: &[u8]) -> bool {
    // A single character in braces sends that character literally — `{a}`,
    // and crucially the escapes `{!}`, `{+}`, `{^}`, `{#}`, `{{}`, `{}}`.
    if name.len() == 1 {
        return true;
    }
    let mut i = 0;
    while i < NAMED_KEYS.len() {
        if eq_ascii_ci(name, NAMED_KEYS[i].as_bytes()) {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether the optional part after a key name is a valid repeat or hold.
///
/// AutoIt accepts `{TAB 4}` (repeat), `{a down}` and `{a up}` (hold/release).
const fn is_valid_suffix(suffix: &[u8]) -> bool {
    if suffix.is_empty() {
        return false;
    }
    if eq_ascii_ci(suffix, b"down") || eq_ascii_ci(suffix, b"up") {
        return true;
    }
    let mut i = 0;
    while i < suffix.len() {
        if !suffix[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `s` is a well-formed AutoIt send sequence.
///
/// Usable in `const` context, which is what lets [`keys!`](macro@crate::keys) reject a bad literal
/// at compile time. [`Keys::parse`] applies exactly the same rules at run time
/// — a property test holds the two in agreement.
#[must_use]
pub const fn validate(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'{' {
            // Everything else is literal, including a bare `}`, which AutoIt
            // passes through. Modifier shorthand (`!+^#`) is likewise fine
            // anywhere.
            i += 1;
            continue;
        }

        // `{{}` and `{}}` are the escapes for the braces themselves; treat the
        // character right after `{` as content, so the scan for `}` starts past
        // it and cannot mistake `{}}`'s first `}` for the terminator.
        let start = i + 1;
        if start >= b.len() {
            return false; // trailing '{'
        }
        let mut j = start + 1;
        while j < b.len() && b[j] != b'}' {
            j += 1;
        }
        if j >= b.len() {
            return false; // unterminated
        }

        // Split the contents on the first space: name, then optional suffix.
        let mut sp = start;
        while sp < j && b[sp] != b' ' {
            sp += 1;
        }

        let name = split(b, start, sp);
        if !is_known_name(name) {
            return false;
        }

        if sp < j {
            // Skip the run of spaces, then validate the suffix.
            let mut k = sp;
            while k < j && b[k] == b' ' {
                k += 1;
            }
            if !is_valid_suffix(split(b, k, j)) {
                return false;
            }
        }

        i = j + 1;
    }

    true
}

/// `&b[from..to]`, written so it is usable in a `const fn`.
const fn split(b: &[u8], from: usize, to: usize) -> &[u8] {
    // `split_at` is const-stable and avoids range-index in const context.
    let (_, rest) = b.split_at(from);
    let (mid, _) = rest.split_at(to - from);
    mid
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A validated keystroke sequence, ready for `Send`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keys(Cow<'static, str>);

impl Keys {
    /// Sends `s` as literal text, escaping everything `Send` would interpret.
    ///
    /// **This is the one to reach for with data.** Whatever is in the string —
    /// a customer name, a currency amount, a password — arrives as itself.
    ///
    /// ```
    /// # use autoitx::Keys;
    /// // Braces arrive as braces, not as a key command.
    /// assert_eq!(Keys::text("a{b}c").as_str(), "a{{}b{}}c");
    /// ```
    #[must_use]
    pub fn text(s: &str) -> Self {
        // Fast path: most text contains none of the six.
        if !s.contains(SPECIAL_CHARS) {
            return Self(Cow::Owned(s.to_owned()));
        }
        let mut out = String::with_capacity(s.len() + 8);
        for c in s.chars() {
            match c {
                '{' => out.push_str("{{}"),
                '}' => out.push_str("{}}"),
                '!' => out.push_str("{!}"),
                '+' => out.push_str("{+}"),
                '^' => out.push_str("{^}"),
                '#' => out.push_str("{#}"),
                _ => out.push(c),
            }
        }
        Self(Cow::Owned(out))
    }

    /// Parses a send sequence, validating its syntax.
    ///
    /// # Errors
    ///
    /// [`KeyParseError`] describing what is wrong and where.
    pub fn parse(s: &str) -> Result<Self, KeyParseError> {
        tokenize(s)?;
        Ok(Self(Cow::Owned(s.to_owned())))
    }

    /// Wraps a literal already checked by [`keys!`](macro@crate::keys).
    ///
    /// Not meant to be called directly — [`keys!`](macro@crate::keys) uses it after its `const`
    /// assertion. Calling it with an unvalidated string is not unsafe, but
    /// produces a sequence `Send` may interpret unexpectedly.
    #[must_use]
    #[doc(hidden)]
    pub const fn from_literal(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }

    /// Takes a sequence as-is, with no validation.
    ///
    /// For genuinely dynamic sequences whose syntax you have established some
    /// other way. Deliberately verbose: reviewers should notice it.
    #[must_use]
    pub fn raw_unchecked(s: impl Into<Cow<'static, str>>) -> Self {
        Self(s.into())
    }

    /// The underlying send string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Concatenates two sequences.
    ///
    /// ```
    /// # use autoitx::Keys;
    /// let login = Keys::text("rpa.user").then(Keys::parse("{TAB}")?);
    /// assert_eq!(login.as_str(), "rpa.user{TAB}");
    /// # Ok::<(), autoitx::keys::KeyParseError>(())
    /// ```
    #[must_use]
    pub fn then(self, other: Self) -> Self {
        Self(Cow::Owned(format!("{}{}", self.0, other.0)))
    }

    /// The parsed token stream.
    ///
    /// Windows passes [`as_str`](Self::as_str) straight to `AU3_Send`; the
    /// macOS backend needs the tokens to synthesise events.
    ///
    /// # Errors
    ///
    /// [`KeyParseError`] if the sequence came from [`raw_unchecked`](Self::raw_unchecked)
    /// and is malformed. Sequences from the other constructors always tokenize.
    pub fn tokens(&self) -> Result<Vec<Token>, KeyParseError> {
        tokenize(&self.0)
    }
}

impl std::fmt::Display for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// One element of a parsed send sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Token {
    /// A character typed as itself.
    Char(char),
    /// A named key, uppercased, with an optional repeat count.
    Named {
        /// The key name, normalised to upper case.
        name: String,
        /// How many times to send it. `1` unless `{KEY n}` said otherwise.
        repeat: u32,
        /// `Some(true)` to hold down, `Some(false)` to release, from
        /// `{KEY down}` / `{KEY up}`.
        hold: Option<bool>,
    },
    /// A shorthand modifier (`!` `+` `^` `#`) applying to the next key.
    Modifier(Modifier),
}

/// A modifier expressed in shorthand form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// `!`
    Alt,
    /// `+`
    Shift,
    /// `^`
    Ctrl,
    /// `#` — the Windows key, or Command on macOS.
    Win,
}

/// Why a send sequence could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyParseError {
    /// A `{` with no matching `}`.
    #[error("unterminated '{{' at byte {at}")]
    Unterminated {
        /// Byte offset of the opening brace.
        at: usize,
    },
    /// A name that is not a known key.
    #[error("unknown key name {name:?} at byte {at} — see autoitx::keys for the vocabulary")]
    UnknownKey {
        /// The name as written.
        name: String,
        /// Byte offset of the opening brace.
        at: usize,
    },
    /// A `{KEY x}` suffix that is neither a count nor `down`/`up`.
    #[error("invalid repeat/hold {suffix:?} at byte {at} — expected a number, 'down', or 'up'")]
    InvalidSuffix {
        /// The suffix as written.
        suffix: String,
        /// Byte offset of the opening brace.
        at: usize,
    },
}

/// Parses a send sequence into tokens.
fn tokenize(s: &str) -> Result<Vec<Token>, KeyParseError> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < b.len() {
        match b[i] {
            b'{' => {
                let at = i;
                let start = i + 1;
                if start >= b.len() {
                    return Err(KeyParseError::Unterminated { at });
                }
                // Start past the first content byte so `{{}` and `{}}` work.
                let mut j = start + 1;
                while j < b.len() && b[j] != b'}' {
                    j += 1;
                }
                if j >= b.len() {
                    return Err(KeyParseError::Unterminated { at });
                }

                let inner = &s[start..j];
                let (name, suffix) = match inner.find(' ') {
                    Some(sp) => (&inner[..sp], inner[sp..].trim_start()),
                    None => (inner, ""),
                };

                if !is_known_name(name.as_bytes()) {
                    return Err(KeyParseError::UnknownKey {
                        name: name.to_owned(),
                        at,
                    });
                }

                let (repeat, hold) = if suffix.is_empty() {
                    (1, None)
                } else if suffix.eq_ignore_ascii_case("down") {
                    (1, Some(true))
                } else if suffix.eq_ignore_ascii_case("up") {
                    (1, Some(false))
                } else {
                    let n = suffix
                        .parse::<u32>()
                        .map_err(|_| KeyParseError::InvalidSuffix {
                            suffix: suffix.to_owned(),
                            at,
                        })?;
                    (n, None)
                };

                // A single character in braces is that character, not a name —
                // this is how `{!}` and `{{}` escape.
                let mut chars = name.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => out.push(Token::Char(c)),
                    _ => out.push(Token::Named {
                        name: name.to_ascii_uppercase(),
                        repeat,
                        hold,
                    }),
                }

                i = j + 1;
            }
            b'!' => {
                out.push(Token::Modifier(Modifier::Alt));
                i += 1;
            }
            b'+' => {
                out.push(Token::Modifier(Modifier::Shift));
                i += 1;
            }
            b'^' => {
                out.push(Token::Modifier(Modifier::Ctrl));
                i += 1;
            }
            b'#' => {
                out.push(Token::Modifier(Modifier::Win));
                i += 1;
            }
            _ => {
                // Step by whole characters so non-ASCII text is preserved.
                let c = s[i..].chars().next().expect("index is a char boundary");
                out.push(Token::Char(c));
                i += c.len_utf8();
            }
        }
    }

    Ok(out)
}

/// Builds a [`Keys`] from a string literal, validated at compile time.
///
/// ```
/// # use autoitx::keys;
/// let copy = keys!("{CTRLDOWN}c{CTRLUP}");
/// assert_eq!(copy.as_str(), "{CTRLDOWN}c{CTRLUP}");
/// ```
///
/// A malformed literal is a build error, not a runtime surprise:
///
/// ```compile_fail
/// # use autoitx::keys;
/// let oops = keys!("{CTRLDOWN");
/// ```
#[macro_export]
macro_rules! keys {
    ($s:literal) => {{
        // The message must not contain braces: `assert!` treats it as a
        // format string, and a literal brace there is a confusing build error
        // about format placeholders rather than about the key sequence.
        const _: () = assert!(
            $crate::keys::validate($s),
            "invalid AutoIt key sequence: check for an unclosed brace or an unknown key name",
        );
        $crate::keys::Keys::from_literal($s)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escapes_every_special_character() {
        assert_eq!(Keys::text("{").as_str(), "{{}");
        assert_eq!(Keys::text("}").as_str(), "{}}");
        assert_eq!(Keys::text("!").as_str(), "{!}");
        assert_eq!(Keys::text("+").as_str(), "{+}");
        assert_eq!(Keys::text("^").as_str(), "{^}");
        assert_eq!(Keys::text("#").as_str(), "{#}");
    }

    #[test]
    fn escaped_text_round_trips_to_the_original_characters() {
        // The property that matters: escaping then tokenizing must give back
        // exactly the input characters, with no command tokens in between.
        for input in [
            "R$ 1.234,56",
            "Order Entry",
            "a{b}c",
            "!+^#{}",
            "password{with}braces!",
            "",
        ] {
            let toks = Keys::text(input).tokens().expect("escaped text must parse");
            let round: String = toks
                .iter()
                .map(|t| match t {
                    Token::Char(c) => *c,
                    other => panic!("escaped text produced a command token: {other:?}"),
                })
                .collect();
            assert_eq!(round, input, "round trip failed for {input:?}");
        }
    }

    #[test]
    fn text_cannot_smuggle_a_command_through_data() {
        // The exact shape of the bug in the .NET code: a password that happens
        // to contain send syntax.
        let hostile = "{ENTER}{CTRLDOWN}a{CTRLUP}!{F4}";
        let toks = Keys::text(hostile).tokens().unwrap();
        assert!(
            toks.iter().all(|t| matches!(t, Token::Char(_))),
            "data became commands: {toks:?}"
        );
    }

    #[test]
    fn the_sequences_the_rpas_actually_send_all_validate() {
        for s in [
            "{TAB}",
            "{ENTER}",
            "{SHIFTDOWN}{TAB}{SHIFTUP}",
            "{CTRLDOWN}c{CTRLUP}",
            "{CTRLDOWN}v{CTRLUP}",
            "{CTRLDOWN}l{CTRLUP}",
            "{CTRLDOWN}{SHIFTDOWN}j{SHIFTUP}{CTRLUP}",
            "{CTRLDOWN}{SHIFTDOWN}w{SHIFTUP}{CTRLUP}",
            "{ALTDOWN}{PRINTSCREEN}{ALTUP}",
            "{ALTDOWN}d{ALTUP}",
            "{LWINDOWN}d{LWINUP}",
            "{END}{SHIFTDOWN}{HOME}{SHIFTUP}",
            "{CTRLDOWN}{END}{CTRLUP}{CTRLDOWN}{SHIFTDOWN}{HOME}{SHIFTUP}{CTRLUP}",
            "{TAB}{TAB}{TAB}{TAB}{TAB}{TAB}{TAB}{SPACE}",
            "{F6}",
            "{PGUP}",
            "{ESC}",
            "{BACKSPACE}",
            "no{ENTER}",
            "01011900",
        ] {
            assert!(validate(s), "should be valid: {s:?}");
            assert!(Keys::parse(s).is_ok(), "should parse: {s:?}");
        }
    }

    #[test]
    fn malformed_sequences_are_rejected() {
        for s in [
            "{CTRLDOWN",
            "{",
            "{TABB}",
            "{TAB 4x}",
            "{NOPE}",
            "{TAB down up}",
        ] {
            assert!(!validate(s), "should be invalid: {s:?}");
            assert!(Keys::parse(s).is_err(), "should not parse: {s:?}");
        }
    }

    #[test]
    fn repeat_and_hold_suffixes_parse() {
        let t = Keys::parse("{TAB 4}").unwrap().tokens().unwrap();
        assert_eq!(
            t,
            vec![Token::Named {
                name: "TAB".into(),
                repeat: 4,
                hold: None
            }]
        );

        let t = Keys::parse("{SHIFT down}").unwrap().tokens().unwrap();
        assert_eq!(
            t,
            vec![Token::Named {
                name: "SHIFT".into(),
                repeat: 1,
                hold: Some(true)
            }]
        );
    }

    #[test]
    fn shorthand_modifiers_tokenize() {
        let t = Keys::parse("^c").unwrap().tokens().unwrap();
        assert_eq!(t, vec![Token::Modifier(Modifier::Ctrl), Token::Char('c')]);
    }

    #[test]
    fn key_names_are_case_insensitive_like_autoit() {
        for s in ["{tab}", "{Tab}", "{TAB}", "{ctrldown}a{CtrlUp}"] {
            assert!(validate(s), "{s:?}");
        }
    }

    #[test]
    fn non_ascii_text_is_preserved_character_by_character() {
        let t = Keys::text("ção").tokens().unwrap();
        assert_eq!(
            t,
            vec![Token::Char('ç'), Token::Char('ã'), Token::Char('o')]
        );
    }

    #[test]
    fn const_validator_agrees_with_the_runtime_parser() {
        // These two must never drift: one gates `keys!` at compile time, the
        // other gates `Keys::parse` at run time.
        for s in [
            "",
            "abc",
            "{TAB}",
            "{TAB 4}",
            "{TAB down}",
            "{!}",
            "{{}",
            "{}}",
            "^c",
            "{CTRLDOWN",
            "{NOPE}",
            "{TAB 4x}",
            "{",
            "}",
            "a}b",
            "{a}",
            "{ASC 065}",
        ] {
            assert_eq!(
                validate(s),
                tokenize(s).is_ok(),
                "validator and parser disagree on {s:?}"
            );
        }
    }

    #[test]
    fn then_concatenates() {
        let k = Keys::text("74").then(Keys::parse("{TAB}").unwrap());
        assert_eq!(k.as_str(), "74{TAB}");
    }
}
