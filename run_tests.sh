#!/bin/bash

# Build programs
cargo build-sbf --manifest-path programs/steward/Cargo.toml;
cargo build-sbf --manifest-path programs/validator-history/Cargo.toml;

# Run all tests
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p tests --all-features --color auto
# SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p stakenet-sdk  --all-features --color auto
# SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p jito-steward --all-features --color auto
