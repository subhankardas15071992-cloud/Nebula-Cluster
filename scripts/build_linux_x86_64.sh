#!/usr/bin/env sh
set -eu

if [ "$(uname -s)" != "Linux" ]; then
    printf '%s\n' "This build script is for Linux."
    exit 1
fi

if [ "$(uname -m)" != "x86_64" ]; then
    printf '%s\n' "This build script targets x86_64 Linux."
    exit 1
fi

export CARGO_INCREMENTAL=0
export RUSTFLAGS="-C target-cpu=native"

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release -- --test-threads=1
cargo xtask bundle nebula_cluster --release

printf '%s\n' "Nebula Cluster Linux CLAP and VST3 bundles are in target/bundled."
