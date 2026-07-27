//! Waiting for an application to stop being busy — portably.
//!
//! One intent, two mechanisms. Windows has no way to ask an application
//! whether it is ready, so it reads the cursor shape: the idiom automation
//! writes by hand as `cursor == 2 || cursor == 5`, usually with no timeout at
//! all. macOS can ask directly, by giving an accessibility query a short
//! deadline and seeing whether the application answers.
//!
//! Your code says `wait_until_idle` either way.
//!
//! ```text
//! cargo run --example wait_until_ready
//! ```

use autoitx::{AutoIt, recipes};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ai = AutoIt::new()?;

    println!("waiting for the frontmost application to be ready...");
    let started = Instant::now();

    match recipes::wait_until_idle(&ai, Duration::from_secs(10)) {
        Ok(()) => println!("ready after {:?}", started.elapsed()),
        // A timeout here is information, not a crash: the application is
        // genuinely busy, and typing into it now would drop keystrokes.
        Err(e) => println!("still busy after {:?}: {e}", started.elapsed()),
    }

    #[cfg(target_os = "macos")]
    {
        println!("\nmacOS can also ask about one process by pid:");
        let me = std::process::id() as i32;
        println!(
            "  is_app_responsive({me}) = {}",
            autoitx::ext::macos::is_app_responsive(me, Duration::from_millis(500))
        );
    }

    #[cfg(windows)]
    {
        // Windows-only, and reaching for it in portable code is a compile
        // error on macOS — which is the point of the `ext` split.
        println!("\nWindows can also read the raw cursor shape:");
        println!("  mouse_get_cursor() = {:?}", ai.mouse_get_cursor()?);
    }

    Ok(())
}
