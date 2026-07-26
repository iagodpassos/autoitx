# autoitx-sys

Raw FFI bindings to the AutoItX3 DLL (the `AU3_*` C ABI), loaded at runtime.

This is the unsafe, 1:1 mirror layer. **Most users want [`autoitx`] instead**,
which wraps it in a safe, cross-platform API.

[`autoitx`]: https://crates.io/crates/autoitx

## Why runtime loading

The DLL is opened with `libloading` rather than declared with `#[link]`, which
means there is no link-time dependency on anything Windows. Consequence:

```bash
cargo check --target x86_64-pc-windows-gnu
```

type-checks the whole thing from macOS or Linux — no MSVC toolchain, no mingw,
no import library. It also lets a mock DLL stand in for the real one, so the
marshalling layer is testable on any OS.

## ABI notes

- Everything is `extern "system"`. On x86_64, `WINAPI`/`__stdcall` is a no-op
  alias for the single Microsoft x64 convention, so the same declarations serve
  both `-msvc` and `-gnu`. Symbols are undecorated.
- 32-bit Windows is **not supported**: exports there are stdcall-decorated
  (`_AU3_Init@0`), and the target DLL is x64.
- Strings are UTF-16. Outputs use a caller-allocated buffer plus `nBufSize`,
  counted in wide characters **including the NUL**. AutoItX never reports how
  much room it needed, so callers grow and retry.

## License

MIT or Apache-2.0, at your option. AutoIt and AutoItX are products of AutoIt
Consulting Ltd; this project is not affiliated with them and does not
distribute their DLL. See `NOTICE` in the repository root.
