//! Reports whether this machine can run automation at all, and why not.
//!
//! Run this first when something is wrong. The two failures that cost the most
//! time both look like something else:
//!
//! - On Windows, a missing or 32-bit `AutoItX3_x64.dll` fails at *load*, and
//!   the error names a symbol rather than a file.
//! - On macOS, a missing Accessibility grant makes every window query return
//!   "not found", so you go hunting for a selector bug that is not there.
//!
//! ```text
//! cargo run --example diagnose
//! ```

fn main() {
    println!("autoitx {}\n", env!("CARGO_PKG_VERSION"));
    println!(
        "binary   {}",
        std::env::current_exe().unwrap_or_default().display()
    );
    println!("target   {}\n", target());

    #[cfg(any(windows, feature = "mock-loader"))]
    dll_backend();

    #[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
    native_backend();

    println!("\n-- automation handle --");
    match autoitx::AutoIt::new() {
        Ok(ai) => {
            let o = ai.options();
            println!("ok\n");
            println!("  window title match   {:?}", o.win_title_match_mode);
            println!("  mouse coordinates    {:?}", o.mouse_coord_mode);
            println!("  send key delay       {:?}", o.send_key_delay);
            println!("  key map              {:?}", o.key_map);
            println!("\nThose are AutoIt's own defaults. Existing automation depends");
            println!("on them without saying so — prefix title matching especially.");
        }
        Err(e) => {
            println!("FAILED\n");
            println!("  {e}");
        }
    }
}

fn target() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "x86_64 Windows"
    } else if cfg!(windows) {
        "Windows (not x86_64 — the AutoItX DLL is x64 only)"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "unsupported"
    }
}

/// Where the DLL is looked for, in order, and which candidates exist.
#[cfg(any(windows, feature = "mock-loader"))]
fn dll_backend() {
    println!("-- AutoItX3 DLL --");
    println!("Searched in this order; the first hit wins.\n");

    let mut found = false;
    for (n, path) in autoitx_sys::loader::search_paths(None).iter().enumerate() {
        let mark = if path.is_file() {
            found = true;
            "found"
        } else {
            "  -  "
        };
        println!("  {mark}  {}. {}", n + 1, path.display());
    }

    if !found {
        println!("\nNothing found. Set AUTOITX_DLL to the file, or AUTOITX_DIR to");
        println!("its folder. The DLL ships with AutoIt itself and is not");
        println!("redistributed with this crate — see NOTICE.");
    }
}

/// Which privacy grants this exact binary holds.
#[cfg(all(target_os = "macos", not(feature = "mock-loader")))]
fn native_backend() {
    use autoitx::ext::macos::{Permission, PermissionStatus, check};

    println!("-- macOS privacy grants --");
    println!("Tied to this binary's path and signature, not to the project.\n");

    for permission in [Permission::Accessibility, Permission::ScreenRecording] {
        let status = check(permission);
        let mark = if status == PermissionStatus::Granted {
            "granted"
        } else {
            "MISSING"
        };
        println!("  {mark}  {permission:?}");
        if status != PermissionStatus::Granted {
            println!("           {}", permission.hint());
        }
    }
    println!("\nAccessibility is required for windows and keystrokes.");
    println!("Screen Recording only for the pixel functions.");
}
