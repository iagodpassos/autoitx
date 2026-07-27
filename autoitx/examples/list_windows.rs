//! Lists what is on screen, so you can write a selector that matches it.
//!
//! The fastest way past "no window matched": see the titles the backend
//! actually sees. On macOS this needs the Accessibility grant — run the
//! `diagnose` example first if the list comes back empty.
//!
//! ```text
//! cargo run --example list_windows
//! cargo run --example list_windows -- Chrome     # filter by substring
//! ```

use autoitx::{AutoIt, Selector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = std::env::args().nth(1);
    let ai = AutoIt::new()?;

    // `[REGEXPTITLE:(.*)]` matches everything, which is the point: this asks
    // for whatever is there rather than for something in particular.
    let pattern = filter
        .as_deref()
        .map_or_else(|| "(.*)".to_owned(), |f| format!("(.*){f}(.*)"));
    let any = Selector::from(format!("[REGEXPTITLE:{pattern}]").as_str());

    match ai.win_get_title(&any) {
        Ok(title) => {
            let pid = ai.win_get_process(&any)?;
            let rect = ai.win_get_pos(&any)?;
            println!("first match\n");
            println!("  title  {title:?}");
            println!("  pid    {pid}");
            println!("  rect   {rect:?}");
            println!("\nA selector that finds it again:");
            println!("  Selector::title({:?})", first_words(&title));
        }
        Err(e) => {
            println!("nothing matched: {e}");
            if filter.is_some() {
                println!("\nTry without a filter to see whether anything matches at all.");
            }
        }
    }
    Ok(())
}

/// A prefix short enough to survive the title changing.
///
/// Titles carry a document name, a modified marker, a tab count. Matching on
/// the whole thing is how automation breaks the first time someone opens a
/// second tab; the default title-match mode is prefix for this reason.
fn first_words(title: &str) -> String {
    title
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}
