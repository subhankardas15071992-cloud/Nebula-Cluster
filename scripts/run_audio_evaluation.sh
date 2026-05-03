#!/usr/bin/env sh
set -eu

cargo test --release audio_evaluation -- --test-threads=1 --nocapture
cargo test --release stress -- --test-threads=1 --nocapture
