#!/usr/bin/env sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
    printf '%s\n' "This build script is for macOS."
    exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
    printf '%s\n' "Xcode Command Line Tools are required."
    exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin

export CARGO_INCREMENTAL=0
export MACOSX_DEPLOYMENT_TARGET=11.0

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release -- --test-threads=1
cargo xtask bundle-universal nebula_cluster --release

printf '%s\n' "Nebula Cluster macOS Universal CLAP and VST3 bundles are in target/bundled."
