# autoitx

[![CI](https://github.com/iagodpassos/autoitx/actions/workflows/ci.yml/badge.svg)](https://github.com/iagodpassos/autoitx/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/autoitx.svg)](https://crates.io/crates/autoitx)
[![docs.rs](https://img.shields.io/docsrs/autoitx)](https://docs.rs/autoitx)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://blog.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

**AutoItX's API, in Rust, on Windows *and* macOS.**

Drive other applications' user interfaces: keystrokes, mouse, clipboard,
windows, processes. The API is modeled on [AutoItX], so existing AutoIt
automation ports over almost mechanically — but unlike AutoItX, it also runs
natively on macOS.

> ⚠️ **Status: under construction.** Phase 0 of 6. The API is not yet usable.
> See [the roadmap](#roadmap).

[AutoItX]: https://www.autoitscript.com/site/autoit/

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

## Two things this fixes about hand-written AutoIt code

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

## Setup

### Windows

`autoitx` does not ship the AutoItX DLL — see [NOTICE](NOTICE). Download AutoIt
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

## Roadmap

| Phase | | |
|---|---|---|
| 0 | Workspace, CI, cross-compilation proven | ✅ |
| 1 | `autoitx-sys` — all 117 `AU3_*` bindings + mock DLL | ✅ |
| 2 | Windows safe layer: `Selector`, `Keys`, `Session`, `recipes` | ✅ |
| 3 | Windows: remaining core + `ext::windows` | ✅ |
| 4 | macOS: permissions, clipboard, mouse, keyboard | ✅ |
| 5 | macOS: windows via Accessibility | ✅ |
| 6 | `0.1.0` release | |
| — | Native Windows backend (drops the DLL), then Linux | |

## Support

If this saves you time, you can [buy me a coffee](https://buymeacoffee.com/iagodpassos). ☕

## License

MIT or Apache-2.0, at your option.

AutoIt and AutoItX are products of AutoIt Consulting Ltd. **This project is not
affiliated with, endorsed by, or sponsored by them**, and the AutoItX3 DLL is
not distributed with it. See [NOTICE](NOTICE).
