# Test fixtures

## `au3_exports.txt`

The export table of the real `AutoItX3_x64.dll`, one symbol per line, sorted.
A test asserts that the bindings declared in this crate match it exactly — in
both directions, so neither a typo'd symbol nor a forgotten function slips
through. CI runs that test on every platform; it needs no DLL and no Windows.

**Provenance**

| | |
|---|---|
| Source | `AutoItX.Dotnet` NuGet package, version 3.3.14.5 |
| File | `AutoItX3_x64.dll`, PE32+ x86-64, dated 2018-03-15 |
| SHA-256 | `5c1acd56bf432462e59e05e72d486fad670c4dd7c556df3d3270b827d1bbc555` |
| Symbols | 117 `AU3_*` (plus 4 COM registration exports, excluded) |
| Extracted with | `llvm-objdump -p` |

Note this is 117 functions, not the 127 that some third-party copies of
`AutoItX3_DLL.h` floating around the web imply. The DLL is authoritative.

**Regenerating** (needs a copy of the DLL — see `NOTICE`; it is not in this
repository):

```sh
just snapshot-exports ~/.local/share/autoitx/AutoItX3_x64.dll
```

A diff here means the DLL version changed. That is a deliberate decision, not a
routine update: adding symbols is additive, but removing one breaks every
downstream user whose AutoIt install is older.
