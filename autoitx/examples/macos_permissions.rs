//! Reports which macOS privacy permissions this binary has.
//!
//! The first thing to run when automation on macOS behaves as though every
//! window has vanished: without the Accessibility grant, every Accessibility
//! call fails in a way that is indistinguishable from "no such window".
//!
//! ```text
//! cargo run --example macos_permissions
//! cargo run --example macos_permissions -- --ask   # show the system prompt
//! ```

fn main() {
    #[cfg(target_os = "macos")]
    {
        use autoitx::ext::macos::{Permission, check, request};

        let ask = std::env::args().any(|a| a == "--ask");

        println!(
            "binary: {}",
            std::env::current_exe().unwrap_or_default().display()
        );
        println!(
            "\nGrants follow the binary's path and signature, so this exact file\n\
             is what was allowed or denied — not \"the project\".\n"
        );

        for permission in [Permission::Accessibility, Permission::ScreenRecording] {
            let status = if ask {
                request(permission)
            } else {
                check(permission)
            };
            println!("{permission:<20?} {status:?}");

            if !status.is_granted() {
                println!("  {}\n", Permission::hint(permission).replace('\n', "\n  "));
            }
        }

        if !ask {
            println!("\nRe-run with --ask to show the system prompt.");
        }
    }

    #[cfg(not(target_os = "macos"))]
    println!("This example reports macOS privacy permissions; there are none to report here.");
}
