//! Reading a field off the screen without the clipboard race.
//!
//! Automation reads screens it cannot query by selecting, copying, and reading
//! the clipboard. The hard part is knowing *when* the copy landed. The idiom
//! everyone writes is a sentinel — put a known value on the clipboard, copy,
//! check whether it changed — and it has three failure modes, all of which
//! happen in production.
//!
//! `recipes::read_screen_text` waits on the OS clipboard sequence number,
//! which every write bumps. It cannot collide with a real value, it notices a
//! copy that rewrote the same text, and a copy that never happened is reported
//! instead of returning the previous field's value.
//!
//! ```text
//! cargo run --example read_field
//! ```
//!
//! Focus a text editor with something selectable in it, then run this.

use autoitx::options::{KeyMap, Options};
use autoitx::{AutoIt, keys, recipes};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ai = AutoIt::builder()
        // On macOS, so that {CTRLDOWN}a{CTRLUP} means Select All rather than
        // Control-A. On Windows this changes nothing.
        .options(Options::default().with_key_map(KeyMap::PortableShortcuts))
        .build()?;

    println!("Focus a text editor. Reading its contents in 4 seconds...\n");
    std::thread::sleep(Duration::from_secs(4));

    println!(
        "active window: {:?}",
        ai.win_get_title(&autoitx::Selector::active())?
    );

    match recipes::read_screen_text(&ai, keys!("{CTRLDOWN}a{CTRLUP}"), Duration::from_secs(5)) {
        Ok(text) => {
            println!("\nread back {} characters:", text.chars().count());
            println!("{:?}", text.chars().take(200).collect::<String>());
        }
        Err(e) => {
            // Worth distinguishing: this is "the copy did not happen", not
            // "the field was empty". An empty field still bumps the counter
            // and comes back as Ok("").
            println!("\nnothing reached the clipboard: {e}");
        }
    }
    Ok(())
}
