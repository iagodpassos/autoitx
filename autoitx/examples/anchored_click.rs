//! Clicking a control without pinning the screen resolution.
//!
//! Hand-written AutoIt automation clicks absolute screen coordinates, which is
//! why it also hard-codes a resolution check at startup and refuses to run when
//! the display changes. Anchoring the click to the window removes that: the
//! window can be anywhere, and the offset still lands.
//!
//! ```text
//! cargo run --example anchored_click
//! ```
//!
//! Prints where it *would* click rather than clicking, so it is safe to run.

use autoitx::{AutoIt, Selector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ai = AutoIt::new()?;
    let window = Selector::active();

    let rect = ai.win_get_pos(&window)?;
    println!("active window at {rect:?}\n");

    // The offset is measured from the window's top-left corner, so it survives
    // the window being moved, the display changing, and a second monitor.
    for (label, dx, dy) in [("OK button", 600, 420), ("first grid cell", 120, 180)] {
        let point = rect.point_at(dx, dy);
        println!("  {label:16} offset ({dx}, {dy}) -> screen {point:?}");
    }

    println!("\nrecipes::click_in_window(&ai, &window, 600, 420) clicks the first");
    println!("of those, re-measuring the window immediately before it does — so");
    println!("a window that moved between the two calls cannot mislead it.");

    println!("\nThe absolute-coordinate equivalent, for comparison:");
    println!(
        "  ai.mouse_click(Point::new({}, {}))",
        rect.x + 600,
        rect.y + 420
    );
    println!("  ...which is correct exactly until something moves.");
    Ok(())
}
