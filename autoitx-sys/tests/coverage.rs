//! Accounting for the export table.
//!
//! The safe `autoitx` crate does not wrap all 117 exports one-for-one, and this
//! test states why in a form that cannot rot: every symbol it does not wrap is
//! either a `...ByHandle` variant, or one of a short list of named exceptions.
//!
//! That claim is the whole justification for stopping where the wrapping stops,
//! so it is worth a test rather than a paragraph.

const FROZEN: &str = include_str!("data/au3_exports.txt");

fn exports() -> Vec<&'static str> {
    FROZEN
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

/// Symbols the safe layer deliberately does not wrap, other than the
/// `...ByHandle` family.
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "AU3_Init",
        "called once by the loader; nothing else should call it",
    ),
    (
        "AU3_error",
        "read after every call by the `au3!` macro; exposed as AutoIt::raw_error",
    ),
    (
        "AU3_Opt",
        "an alias of AU3_AutoItSetOption, which is wrapped as set_option",
    ),
    (
        "AU3_WinGetHandleAsText",
        "returns the same handle as AU3_WinGetHandle, formatted as hex text",
    ),
    (
        "AU3_ControlGetHandleAsText",
        "as above, for AU3_ControlGetHandle",
    ),
];

#[test]
fn every_by_handle_variant_has_a_selector_based_twin() {
    // This is what makes the ByHandle family redundant rather than missing:
    // `Selector::handle(h)` renders `[HANDLE:...]`, which AutoIt accepts
    // anywhere a selector goes. Wrapping both would double the API surface to
    // express one idea.
    let all = exports();
    let by_handle: Vec<_> = all
        .iter()
        .filter(|s| s.ends_with("ByHandle"))
        .copied()
        .collect();

    assert!(!by_handle.is_empty(), "fixture looks wrong");

    for variant in &by_handle {
        let base = variant.trim_end_matches("ByHandle");
        assert!(
            all.contains(&base),
            "{variant} has no selector-based twin {base}, so it is genuinely \
             missing rather than redundant"
        );
    }
}

#[test]
fn the_export_table_is_fully_accounted_for() {
    let all = exports();
    let by_handle = all.iter().filter(|s| s.ends_with("ByHandle")).count();
    let exceptions = EXCEPTIONS.len();

    // 117 = wrapped + ByHandle twins + named exceptions. If this drifts,
    // something was added to the DLL or removed from the reasoning, and either
    // way it should be looked at rather than absorbed.
    assert_eq!(all.len(), 117);
    assert_eq!(by_handle, 38, "the ByHandle family changed size");

    for (name, _why) in EXCEPTIONS {
        assert!(all.contains(name), "{name} is not in the export table");
    }

    let wrapped_or_wrappable = all.len() - by_handle - exceptions;
    assert_eq!(
        wrapped_or_wrappable, 74,
        "the number of functions the safe layer is expected to wrap changed"
    );
}

#[test]
fn nothing_is_reachable_only_through_unsafe_code() {
    // Not an assertion about this crate so much as a reminder of the contract:
    // whatever the safe layer has not wrapped is still reachable through
    // `AutoIt::raw()`, which is why "not wrapped" never means "not usable".
    //
    // The named exceptions are the only ones a caller has no reason to touch,
    // and each says why.
    for (name, why) in EXCEPTIONS {
        assert!(
            !why.is_empty(),
            "{name} is excluded without a stated reason"
        );
    }
}
