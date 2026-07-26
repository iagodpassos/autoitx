# autoitx — task runner.
#
# The point of this file: the Windows backend is developed and type-checked
# entirely on macOS. `cargo check`/`clippy` never invoke a linker, and the
# AutoItX DLL is loaded at runtime, so there is no link-time Windows
# dependency. A Windows VM is needed only to observe real behaviour.

WIN := "x86_64-pc-windows-gnu"

default:
    @just --list

# The pre-push gate: everything that can be proven without a Windows machine.
check-all: fmt-check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo clippy --target {{WIN}} --all-features -- -D warnings
    cargo test --all-features

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# Type-check the Windows backend from macOS. No linker, no mingw, no VM.
check-win:
    cargo check --target {{WIN}} --all-features

# Exercise the AU3 FFI layer against the mock DLL, on this Mac.
test-mock:
    cargo build -p xtask-mock-dll
    cargo test -p autoitx-sys --features mock-loader

docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --open

# Render docs the way docs.rs will, with platform badges. Needs nightly.
docs-rs:
    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --no-deps --all-features

# Produce a real .exe. Needs `brew install mingw-w64`.
build-win:
    cargo build --release --target {{WIN}}

# Freeze the real DLL's export table, so CI can catch signature drift.
# Run once, on a machine that has AutoItX3_x64.dll.
snapshot-exports dll:
    llvm-objdump -p {{dll}} | grep -oE 'AU3_[A-Za-z_]+' | sort -u \
        > autoitx-sys/tests/data/au3_exports.txt
    @wc -l < autoitx-sys/tests/data/au3_exports.txt
