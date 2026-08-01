# autoitx

[![CI](https://github.com/iagodpassos/autoitx/actions/workflows/ci.yml/badge.svg)](https://github.com/iagodpassos/autoitx/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/autoitx.svg)](https://crates.io/crates/autoitx)
[![docs.rs](https://img.shields.io/docsrs/autoitx)](https://docs.rs/autoitx)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#user-content-license)

**AutoItX's API, in Rust, on Windows *and* macOS.**

Drive other applications' user interfaces: keystrokes, mouse, clipboard,
windows, processes. The API is modeled on [AutoItX], so existing AutoIt
automation ports over almost mechanically — but unlike AutoItX, it also runs
natively on macOS.

```toml
[dependencies]
autoitx = "0.1"
```

📦 [crates.io](https://crates.io/crates/autoitx) · 📖 [API documentation](https://docs.rs/autoitx) · ☕ [Buy me a coffee](https://buymeacoffee.com/iagodpassos)

[AutoItX]: https://www.autoitscript.com/site/autoit/

## Quickstart

```rust,ignore
use autoitx::{AutoIt, Keys, Selector, keys, recipes};
use std::time::Duration;

let ai = AutoIt::new()?;
let orders = Selector::from("[CLASS:Chrome_WidgetWin_1;TITLE:Acme ERP]");

// Wait for the window, bring it forward, and fill the screen.
recipes::open_and_focus(&ai, &orders, Duration::from_secs(30))?;
ai.maximize(&orders)?;

// Type data. `Keys::text` escapes, so a customer name containing `{`
// is typed rather than executed as a key command.
ai.send(Keys::text(&customer.name))?;
ai.send(keys!("{TAB}"))?;

// Read a field back, waiting on the OS clipboard counter rather than
// on a sentinel that a real value could collide with.
let total = recipes::read_screen_text(
    &ai,
    keys!("{END}{SHIFTDOWN}{HOME}{SHIFTUP}"),
    Duration::from_secs(5),
)?;
```

That compiles unchanged on macOS. What differs — a Win32 class has no macOS
counterpart — is handled by [`SelectorSet`](#user-content-platform-support), not by
`#[cfg]` in your logic.

## Platform support

| | Windows | macOS | Linux |
|---|---|---|---|
| **Backend** | `AutoItX3_x64.dll`, loaded at runtime | native | planned |
| Keystrokes, mouse, clipboard | ✅ | ✅ | — |
| Windows by title / class / regex | ✅ | ✅ (Accessibility) | — |
| Processes, `run` | ✅ | ✅ | — |
| Pixel colour / search | ✅ | ✅ (needs Screen Recording) | — |
| Window text and class list | ✅ | ✅ (the AX tree, and its roles) | — |
| Controls by `ClassNameNN` | ✅ (by HWND) | ❌ no HWND to address | — |
| Cursor shape (`mouse_get_cursor`) | ✅ | ❌ no public API — use `recipes::wait_until_idle` | — |
| Mapped network drives, status bars, `WinSetTrans`, tooltips | ✅ | ❌ | — |

Capabilities that exist on only one platform live in `ext::windows` /
`ext::macos`. Using one from the wrong platform is a **compile error**, not a
runtime surprise — a robot that discovers mid-run it cannot read the cursor has
already half-completed a transaction in someone's ERP.

### One thing to know before porting a shortcut

`{CTRLDOWN}c{CTRLUP}` means Copy on Windows and **Control-C** on macOS, where
Copy is Command-C. The default (`KeyMap::AsWritten`) takes the names literally,
so a Windows shortcut does something else — loudly, rather than silently:

```rust
use autoitx::options::{KeyMap, Options};

let ai = autoitx::AutoIt::builder()
    .options(Options::default().with_key_map(KeyMap::PortableShortcuts))
    .build()?;
```

Swap it only if every one of your `CTRL` sequences is an editing shortcut. If
any of them means Control literally — a terminal's Ctrl-C — leave the default
and translate those call sites by hand.

For operations that both platforms can do by different means, `recipes` gives
one portable call. `wait_until_idle` polls the cursor shape on Windows and
probes the Accessibility message timeout on macOS; your code says
`wait_until_idle`.

## Three things this fixes about hand-written AutoIt code

**Keystroke injection.** `Send` interprets `{}!+^#`, so interpolating user or
database data straight into a send string lets that data execute as key
commands — a password containing `{` is a live bug, not a theoretical one.
Here, `Keys::text()` escapes by default, and the raw form has to be asked for
by name.

```rust,ignore
auto.send_text(&password)?;          // always literal, whatever is in it
auto.send(keys!("{CTRLDOWN}c{CTRLUP}"))?;  // validated at compile time
```

**Reading the screen through the clipboard.** The usual idiom — put a sentinel
value on the clipboard, send Ctrl+C, then check whether it changed — races with
anything else touching the clipboard. `recipes::read_screen_text` waits on the
OS clipboard *sequence number* instead, which cannot race.

**Racing several outcomes with no timeout.** An action in a legacy application
can end several ways — the form closes, or an error dialog appears, or a
different dialog appears — and AutoIt only waits on one window at a time, so
the race gets written by hand and reliably forgets the timeout:

```csharp
while (WinExists("Order Selection")
    && !WinExists("[CLASS:ui60Modal_W32]")
    && !WinExists("Blocked")) { Thread.Sleep(300); }   // hangs if none happen
```

`wait_for_any` takes the timeout as a parameter and tells you *which* outcome
happened, so there is no version of the call that can hang.

```rust,ignore
match ai.wait_for_any(&[
    (&orders,  WinCondition::Gone),
    (&modal,   WinCondition::Exists),
    (&blocked, WinCondition::Exists),
], Some(Duration::from_secs(60)))? {
    Some(0) => saved(),
    Some(1) => report_error()?,
    Some(2) => report_blocked()?,
    _ => return Err(wedged()),
}
```

## Setup

### Windows

`autoitx` does not ship the AutoItX DLL — see [NOTICE][notice]. Download AutoIt
from [autoitscript.com](https://www.autoitscript.com/site/autoit/downloads/) and
point the library at `AutoItX3_x64.dll`, which is searched for in this order:

1. `AutoItBuilder::dll_path(..)`
2. `$AUTOITX_DLL` (full path to the file)
3. `$AUTOITX_DIR` (directory containing it)
4. next to your executable
5. the current working directory
6. the registry (`HKLM\SOFTWARE\AutoIt v3\AutoIt`)
7. whatever `LoadLibraryW` finds on `PATH`

If none hit, the error lists every path tried. 64-bit only: the DLL is x64, so
32-bit targets are unsupported.

**Windows on ARM works** — build an x86-64 binary and let Windows emulate it.
Confirmed on an ARM64 Windows 11 VM running a full automation flow: the
emulated process is x64, so the x64 DLL loads into it normally. What does *not*
work is building for `aarch64-pc-windows-msvc`, since a native ARM64 process
cannot load an x64 DLL — `Au3::load` reports that specifically rather than as a
generic "not found".

### macOS

Two privacy permissions, requested only when first needed:

- **Accessibility** — all window and control operations.
- **Screen Recording** — pixel and capture operations only.

Grants are keyed to a binary's path *and* code signature. `cargo build` rewrites
the binary, and every `cargo test` run produces a fresh hash-suffixed one, so
macOS will re-prompt constantly during development. Grant the permission to your
terminal or IDE (children inherit it), or ad-hoc sign with
`codesign -s - --force`.

## Developing on a Mac, shipping to Windows

The DLL is loaded at runtime, so there is no link-time Windows dependency, and
`cargo check`/`clippy` never invoke a linker. The whole Windows backend is
therefore type-checked, linted, and unit-tested from macOS:

```bash
rustup target add x86_64-pc-windows-gnu
cargo clippy --target x86_64-pc-windows-gnu --all-features -- -D warnings
just test-mock   # exercises the AU3 FFI layer against a mock DLL, on macOS
```

A real `.exe` needs `brew install mingw-w64` (or `cargo-xwin` for the MSVC ABI).
A Windows machine is needed only to observe real behaviour — never to compile.

## Examples

Eight, in [`autoitx/examples`][examples]. Run any with
`cargo run --example <name>`.

| | |
|---|---|
| [`diagnose`][diagnose] | **Run this first when something is wrong.** The DLL search order with a mark against each candidate; on macOS, which privacy grants this exact binary holds. Also published as a prebuilt binary on each [release](https://github.com/iagodpassos/autoitx/releases). |
| [`list_windows`][list_windows] | What is on screen, so you can write a selector that matches it |
| [`type_safely`][type_safely] | The four ways to build a key sequence, and when each is right |
| [`read_field`][read_field] | Reading a field through the clipboard without the race |
| [`anchored_click`][anchored_click] | Clicking without pinning the screen resolution |
| [`wait_until_ready`][wait_until_ready] | One intent, two mechanisms, one call |
| [`portable_selectors`][portable_selectors] | One selector table for both platforms |
| [`port_from_csharp`][port_from_csharp] | The same flow in AutoItX.Dotnet and here, side by side |

[examples]: https://github.com/iagodpassos/autoitx/tree/main/autoitx/examples
[diagnose]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/diagnose.rs
[list_windows]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/list_windows.rs
[type_safely]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/type_safely.rs
[read_field]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/read_field.rs
[anchored_click]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/anchored_click.rs
[wait_until_ready]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/wait_until_ready.rs
[portable_selectors]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/portable_selectors.rs
[port_from_csharp]: https://github.com/iagodpassos/autoitx/blob/main/autoitx/examples/port_from_csharp.rs

## Status

`0.1.2` — Windows complete, macOS complete for everything with a public API.
The [platform matrix](#user-content-platform-support) is the honest statement of
what works where; nothing in it is aspirational.

Verified against reality rather than only against tests: the Windows backend
was audited by calling every function against a live desktop and recording what
it actually returns on failure (that table is at the top of `backend/dll.rs`,
because the information exists nowhere else), and a full automation flow was
run on a Windows VM. The macOS backend has a live suite that drives real
applications, which is where four bugs a mock could never have caught turned
up — see the [0.1.0 release notes](https://github.com/iagodpassos/autoitx/releases/tag/v0.1.0).

**Next:** a native Windows backend behind the same API, which drops the DLL
dependency entirely. Then Linux (X11 and AT-SPI), then capture and OCR.

## FAQ

**Do I need AutoIt installed?**
On Windows, you need `AutoItX3_x64.dll` — it ships with AutoIt and with the
standalone AutoItX download. It is *not* redistributed here: AutoIt is freeware
under a EULA, not an open-source licence, so shipping it inside a crate would
be a licensing problem rather than a convenience. On macOS nothing is needed;
the backend is native.

**Is this affiliated with AutoIt?**
No. AutoIt and AutoItX are products of AutoIt Consulting Ltd. This project is
independent and unendorsed — see `NOTICE`.

**Why is my macOS build not finding any windows?**
Almost always the Accessibility grant. Without it, every accessibility call
fails in a way indistinguishable from "no such window". Run
`cargo run --example diagnose`.

**Why does macOS keep re-asking for permission?**
Grants are keyed to a binary's path *and* code signature, and every
`cargo build` writes a new binary. Grant the permission to your terminal or
IDE, which children inherit, or ad-hoc sign with `codesign -s - --force`.

**Can I run this on Windows ARM?**
Yes, as an x86-64 binary under emulation — confirmed with a full flow on an
ARM64 Windows 11 VM. A native `aarch64-pc-windows-msvc` build cannot work,
because an ARM64 process cannot load an x64 DLL, and `Au3::load` says so
specifically.

**Does it work on Linux?**
Not yet. X11 via `x11rb` and AT-SPI via `zbus` are the plan.

**Why `Keys::text` instead of just passing a string?**
Because `Send` interprets `{}!+^#`. A price, a name, or a password containing
one of those becomes a key command. `Keys::text` escapes; `keys!` validates at
compile time; `Keys::raw_unchecked` exists but has to be named.

**Is it thread-safe?**
`AutoIt` is `Send + Sync + Clone`, and every call takes a lock. That is not
enough on its own — two flows alternating activate-then-send still fight over
focus — so `ai.session()` holds the lock across a run of calls and forwards the
whole API by `Deref`.

## Support

If this saves you time, you can [buy me a coffee](https://buymeacoffee.com/iagodpassos). ☕

## License

MIT or Apache-2.0, at your option.

AutoIt and AutoItX are products of AutoIt Consulting Ltd. **This project is not
affiliated with, endorsed by, or sponsored by them**, and the AutoItX3 DLL is
not distributed with it. See [NOTICE][notice].

[notice]: https://github.com/iagodpassos/autoitx/blob/main/NOTICE
