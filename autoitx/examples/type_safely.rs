//! The four ways to build a key sequence, and when each is right.
//!
//! This is the security-relevant part of the crate. `Send` interprets
//! `{}!+^#`, so interpolating a name, a price, or a password straight into a
//! send string lets that data execute as key commands.
//!
//! ```text
//! cargo run --example type_safely
//! ```

use autoitx::{Keys, keys};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Data. Escaped, always — this is the one to reach for by default.
    //    Note what is in the string: braces, a plus, an exclamation mark. Every
    //    one of them is a command character to AutoIt.
    let from_the_database = "Ünïcödé {not a token} 50%+VAT!";
    let safe = Keys::text(from_the_database);
    println!("text()  {:?}", safe.as_str());
    println!(
        "        -> {} tokens, all literal characters",
        safe.tokens()?.len()
    );

    // 2. A literal sequence you wrote yourself. Validated at *compile* time:
    //    a typo like {CTRLDWN} does not build.
    let shortcut = keys!("{CTRLDOWN}c{CTRLUP}");
    println!("\nkeys!() {:?}", shortcut.as_str());

    // 3. A sequence assembled at runtime — from config, say. Validated, but
    //    the failure is a Result rather than a build error.
    match Keys::parse("{TAB}{TAB}{ENTER}") {
        Ok(k) => println!("\nparse() {:?} ok", k.as_str()),
        Err(e) => println!("\nparse() rejected: {e}"),
    }
    if let Err(e) = Keys::parse("{NOSUCHKEY}") {
        println!("parse() rejected {{NOSUCHKEY}}: {e}");
    }

    // 4. The escape hatch. Named so it cannot be reached for by accident.
    let raw = Keys::raw_unchecked("{F5}");
    println!(
        "\nraw_unchecked() {:?} — no validation at all",
        raw.as_str()
    );

    println!("\nThe rule: data goes through text(), sequences you wrote go");
    println!("through keys!(). If you find yourself with format!() and a send");
    println!("string, that is the injection bug.");
    Ok(())
}
