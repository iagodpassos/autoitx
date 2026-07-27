//! One selector table for two platforms, without `#[cfg]` in your logic.
//!
//! A Win32 window class has no macOS counterpart: `Chrome_WidgetWin_1` is a
//! class name, `com.google.Chrome` is a bundle identifier. They name the same
//! application by different means, so a selector written for one does not work
//! on the other.
//!
//! `SelectorSet` keeps both in one place and picks at compile time. The
//! alternative is a `#[cfg]` at every call site, which is how platform details
//! end up smeared through business logic.
//!
//! ```text
//! cargo run --example portable_selectors
//! ```

use autoitx::Selector;
use autoitx::selector::SelectorSet;

fn main() {
    let browser = SelectorSet::new(
        Selector::from("[CLASS:Chrome_WidgetWin_1]"),
        Selector::from("[CLASS:com.google.Chrome]"),
    );

    let editor = SelectorSet::new(
        Selector::title("Untitled - Notepad"),
        Selector::title("Untitled"),
    );

    println!(
        "compiled for {}\n",
        if cfg!(windows) { "Windows" } else { "macOS" }
    );

    for (name, set) in [("browser", &browser), ("editor", &editor)] {
        println!("{name}");
        println!("  windows  {}", set.windows());
        println!("  macos    {}", set.macos());
        println!("  current  {}   <- what your code uses", set.current());
    }

    println!("\nEverything else in the API takes `set.current()`, so the flow");
    println!("reads the same on both platforms.");
}
