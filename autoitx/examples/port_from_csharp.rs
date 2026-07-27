//! The same flow in AutoItX.Dotnet and in this crate, side by side.
//!
//! Porting is close to mechanical — the API is deliberately AutoItX's — but
//! three things change, and each one fixes a bug class rather than being a
//! matter of taste.
//!
//! ```text
//! cargo run --example port_from_csharp
//! ```
//!
//! Prints the comparison; drives nothing.

use autoitx::options::{KeyMap, Options};
use autoitx::{AutoIt, Keys, Selector, keys, recipes};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", COMPARISON);

    // The Rust side of the comparison, compiled rather than quoted — so it
    // cannot drift out of date.
    let _ai = AutoIt::builder()
        .options(Options::default().with_key_map(KeyMap::PortableShortcuts))
        .build();

    let window = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme ERP]");
    let _ = &window;

    // 1. Data is escaped. A price with a comma, a name with a brace, a
    //    password with a `+` — none of them become key commands.
    let _typed: Keys = Keys::text("Ünïcödé {literal} 50%+VAT");

    // 2. Sequences you wrote are checked when the crate compiles.
    let _shortcut = keys!("{CTRLDOWN}{SHIFTDOWN}j{SHIFTUP}{CTRLUP}");

    // 3. Reading the screen waits on the clipboard counter, not a sentinel.
    let _read = |ai: &AutoIt| {
        recipes::read_screen_text(
            ai,
            keys!("{END}{SHIFTDOWN}{HOME}{SHIFTUP}"),
            Duration::from_secs(5),
        )
    };

    Ok(())
}

const COMPARISON: &str = r#"
1. Interpolating data into a send string
------------------------------------------------------------------
  C#    AutoItX.Send($"{customer.Name}{{TAB}}{price}");
        // a "{" in the name, a "+" in the price -> key commands

  Rust  ai.send(Keys::text(&customer.name))?;
        ai.send(keys!("{TAB}"))?;
        ai.send(Keys::text(&price))?;
        // text() escapes { } ! + ^ #; raw form must be asked for by name

2. Reading a field through the clipboard
------------------------------------------------------------------
  C#    AutoItX.ClipPut("NO-VALUE");        // sentinel
        AutoItX.Send("^c");
        var v = AutoItX.ClipGet();
        if (v == "NO-VALUE") { /* assume nothing copied */ }
        // breaks when the cell contains the sentinel, when the copy
        // rewrites the same value, and when the copy never happened

  Rust  let v = recipes::read_screen_text(&ai, keys!("^c"), timeout)?;
        // waits on the OS clipboard sequence number; cannot collide

3. Clicking a control
------------------------------------------------------------------
  C#    AutoItX.MouseClick("left", 812, 534);
        // plus a startup check that the screen is 1600x900

  Rust  recipes::click_in_window(&ai, &window, 600, 420)?;
        // offset from the window; no resolution to pin

Everything else maps name for name: WinActivate -> win_activate,
WinWaitActive -> win_wait_active, ProcessClose -> process_close.
The 117 AU3_* entry points are all reachable, and on Windows
AutoIt::raw() reaches the ones without a wrapper.
"#;
