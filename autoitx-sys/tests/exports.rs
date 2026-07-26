//! The bindings must match the real DLL's export table exactly.
//!
//! `tests/data/au3_exports.txt` is a frozen dump of `AutoItX3_x64.dll`'s
//! exports (see the README next to it for provenance). Comparing against it
//! catches the two failure modes a compiler cannot:
//!
//! - a **typo'd symbol**, which would otherwise surface as a runtime
//!   `MissingSymbol` on a user's Windows box;
//! - a **forgotten function**, which would otherwise be invisible.
//!
//! This runs on every platform and needs neither Windows nor the DLL itself.

use autoitx_sys::Au3;
use std::collections::BTreeSet;

const FROZEN: &str = include_str!("data/au3_exports.txt");

fn frozen_exports() -> BTreeSet<&'static str> {
    FROZEN
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

fn declared_symbols() -> BTreeSet<&'static str> {
    Au3::SYMBOLS.iter().copied().collect()
}

#[test]
fn no_binding_is_missing_from_the_dll() {
    let extra: Vec<_> = declared_symbols()
        .difference(&frozen_exports())
        .copied()
        .collect();
    assert!(
        extra.is_empty(),
        "these are declared in api.rs but are NOT exported by AutoItX3_x64.dll \
         — each would fail at runtime with MissingSymbol:\n  {}",
        extra.join("\n  ")
    );
}

#[test]
fn no_dll_export_is_left_unbound() {
    let missing: Vec<_> = frozen_exports()
        .difference(&declared_symbols())
        .copied()
        .collect();
    assert!(
        missing.is_empty(),
        "the DLL exports these but api.rs does not declare them:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_frozen_table_itself_looks_sane() {
    let frozen = frozen_exports();

    // Guards against the fixture being truncated or replaced by something
    // unrelated — a green suite against an empty file would be worthless.
    assert_eq!(frozen.len(), 117, "expected 117 AU3_* exports");
    assert!(frozen.iter().all(|s| s.starts_with("AU3_")));

    // The four COM registration exports are deliberately excluded: they are
    // not part of the automation ABI.
    for com in ["DllRegisterServer", "DllGetClassObject"] {
        assert!(!frozen.contains(com), "{com} should not be in the table");
    }

    // Spot-check the entry points the production RPAs lean on hardest, so a
    // wholesale regeneration mistake cannot pass quietly.
    for essential in [
        "AU3_Init",
        "AU3_error",
        "AU3_Send",
        "AU3_ClipGet",
        "AU3_ClipPut",
        "AU3_MouseClick",
        "AU3_MouseGetCursor",
        "AU3_WinExists",
        "AU3_WinActivate",
        "AU3_WinWaitActive",
        "AU3_WinWaitClose",
        "AU3_WinGetPos",
        "AU3_WinGetProcess",
        "AU3_WinSetState",
    ] {
        assert!(frozen.contains(essential), "{essential} missing from table");
    }
}

#[test]
fn declaration_order_is_stable_and_duplicate_free() {
    // SYMBOLS is generated in declaration order, and the struct's field order
    // follows it. Duplicates would silently shadow a binding.
    assert_eq!(
        Au3::SYMBOLS.len(),
        declared_symbols().len(),
        "api.rs declares the same symbol more than once"
    );
}
