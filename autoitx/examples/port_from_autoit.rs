//! The same automation flow in AutoIt and in this crate, side by side.
//!
//! The compiled Rust below deliberately does not drive an application: this
//! example prints the comparison and exits. Keeping the Rust half as code,
//! rather than only prose, prevents the porting guidance from drifting.
//!
//! ```text
//! cargo run --example port_from_autoit
//! ```

#[cfg(any(windows, target_os = "macos", feature = "mock-loader"))]
use autoitx::options::{KeyMap, Options};
#[cfg(any(windows, target_os = "macos", feature = "mock-loader"))]
use autoitx::{AutoIt, Keys, Selector, keys, recipes};
#[cfg(any(windows, target_os = "macos", feature = "mock-loader"))]
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{COMPARISON}");
    compiled_port()?;
    Ok(())
}

// The Rust side is compiled rather than quoted, but never invoked against a
// real application. The empty Linux version lets this documentation example
// compile and print everywhere.
#[cfg(any(windows, target_os = "macos", feature = "mock-loader"))]
fn compiled_port() -> Result<(), autoitx::Error> {
    if false {
        let ai = AutoIt::builder()
            .options(Options::default().with_key_map(KeyMap::PortableShortcuts))
            .build()?;
        let window = Selector::from("[TITLE:Order Selection]");
        let customer_name = "Ada {priority}+";

        // 1. Dynamic text is data, not an AutoIt key sequence.
        ai.send(Keys::text(customer_name))?;

        // 2. The shortcut is a literal sequence validated at compile time.
        let copied =
            recipes::read_screen_text(&ai, keys!("{CTRLDOWN}c{CTRLUP}"), Duration::from_secs(5))?;
        println!("copied total: {copied}");

        // 3. This offset follows the selected window rather than the screen.
        recipes::click_in_window(&ai, &window, 420, 260)?;

        // 4. Unlike AutoIt's unbounded WinWaitActive call, the timeout is explicit.
        let active = ai.win_wait_active(&window, Some(Duration::from_secs(10)))?;
        println!("order selection active: {active}");
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos", feature = "mock-loader")))]
fn compiled_port() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

const COMPARISON: &str = r#"
1. Waiting for the order-selection window
------------------------------------------------------------------
  AutoIt  WinWaitActive("Order Selection")
           ; waits forever when the application never appears

  Rust    ai.win_wait_active(&window, Some(Duration::from_secs(10)))?;
           // the deadline is explicit, so a missing window is observable

2. Copying a total from the screen
------------------------------------------------------------------
  AutoIt  Send("{CTRLDOWN}c{CTRLUP}")
          $total = ClipGet()
           ; a blind copy can read the previous clipboard value

  Rust    let total = recipes::read_screen_text(
              &ai, keys!("{CTRLDOWN}c{CTRLUP}"), timeout
          )?;
           // waits for the clipboard sequence number to change

3. Typing a customer name
------------------------------------------------------------------
  AutoIt  Send($customerName)
           ; { } ! + ^ # in data are interpreted as key commands

  Rust    ai.send(Keys::text(customer_name))?;
           // text() escapes data; keys!() is for literal, checked sequences

4. Clicking the selected order
------------------------------------------------------------------
  AutoIt  MouseClick("left", 420, 260)
           ; absolute coordinates break when the window moves or the display changes

  Rust    recipes::click_in_window(&ai, &window, 420, 260)?;
           // measures the window and clicks at an offset from its top-left corner

The example prints this comparison only. Its Rust half is compiled to keep the
API names, shortcut syntax, and safe-porting guidance current.
"#;
