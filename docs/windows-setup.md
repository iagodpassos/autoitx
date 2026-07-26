# Windows setup

`autoitx` needs `AutoItX3_x64.dll` at runtime on Windows. It is **not** bundled,
and is not a Cargo dependency — see [`NOTICE`](../NOTICE) for why.

## Getting the DLL

Either:

- **Install AutoIt.** Download from
  [autoitscript.com](https://www.autoitscript.com/site/autoit/downloads/) and
  install the full package. The DLL lands in `AutoItX\AutoItX3_x64.dll` under
  the install directory, and the registry entry makes it discoverable
  automatically.
- **Extract it from the NuGet package.** `AutoItX.Dotnet` on nuget.org is a zip;
  the DLL is at `build/x64/AutoItX3_x64.dll`. Useful if you already vendor it
  for a .NET project. Ignore `AutoItX3.Assembly.dll` — that is the managed .NET
  wrapper, which `autoitx` replaces and does not use.

Verify you have the 64-bit build:

```sh
file AutoItX3_x64.dll
# AutoItX3_x64.dll: PE32+ executable (DLL) (GUI) x86-64, for MS Windows
```

`AutoItX3.dll` (no suffix) is the 32-bit build and will not load. 32-bit targets
are unsupported.

## Where to put it

Searched in this order; the first hit wins, and the error message lists every
path tried if none do:

| | Location |
|---|---|
| 1 | `AutoItBuilder::dll_path(..)` — explicit, no fallback |
| 2 | `$AUTOITX_DLL` — full path to the file |
| 3 | `$AUTOITX_DIR` — directory containing it |
| 4 | Next to your executable |
| 5 | The current working directory |
| 6 | Registry: `HKLM\SOFTWARE\AutoIt v3\AutoIt` → `InstallDir` |
| 7 | `LoadLibraryW("AutoItX3_x64.dll")` — `PATH` and SxS |

For deployment, **4** is usually right: ship the DLL alongside your `.exe`, the
same way the AutoItX NuGet package does for .NET.

For development from a Mac, keep it outside the source tree — the repository
gitignores `*.dll` precisely so it never gets committed:

```sh
mkdir -p ~/.local/share/autoitx
# copy AutoItX3_x64.dll there, then:
export AUTOITX_DIR=~/.local/share/autoitx
```

## Windows on ARM

`AutoItX3_x64.dll` is x64 and cannot load into an ARM64 process. Build for
`x86_64-pc-windows-msvc` and run under emulation. `Au3::load()` reports this
specifically rather than as a generic "not found".

## Licensing

The DLL is AutoIt Consulting Ltd's, under their EULA — not this project's
licence. AutoIt is freeware, but note that **commercial use requires
acknowledging AutoIt in your product and linking to the AutoIt homepage**. Read
their terms before shipping.
