# autoitx — task runner.
#
# The point of this file: the Windows backend is developed and type-checked
# entirely on macOS. `cargo check`/`clippy` never invoke a linker, and the
# AutoItX DLL is loaded at runtime, so there is no link-time Windows
# dependency. A Windows VM is needed only to observe real behaviour.

WIN := "x86_64-pc-windows-gnu"
MSRV := "1.85.0"

default:
    @just --list

# The pre-push gate: everything that can be proven without a Windows machine.
#
# Runs clippy with no features as well as all: the platform and mock-loader
# gates mean code can be dead in one configuration and live in another, and
# `-D warnings` in CI turns that into a failure.
#
# `RUSTFLAGS` matches the CI workflow deliberately. Without it `cargo test`
# only warns about dead code that CI rejects, and this gate passes on something
# that then fails on push — which is exactly what it happened to do.
export RUSTFLAGS := "-D warnings"

check-all: fmt-check msrv
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo clippy --target {{WIN}} -p autoitx-sys -p autoitx --all-features -- -D warnings
    # Both backends. On macOS `--all-features` turns on `mock-loader`, which
    # puts the DLL backend in charge — so without this line the native backend
    # would never be tested here at all.
    cargo test --workspace
    cargo test --workspace --all-features

# Language features are not gated by `rust-version`, so only a real 1.85
# toolchain catches an accidental use of something newer (let chains, say).
#   rustup toolchain install {{MSRV}} --profile minimal
msrv:
    cargo +{{MSRV}} check --workspace --all-features

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
