#!/bin/bash

# Build programs
cargo build-sbf --manifest-path programs/steward/Cargo.toml;
cargo build-sbf --manifest-path programs/validator-history/Cargo.toml;

# Run all tests
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 RUST_LOG=info cargo test --package tests --all-features --color auto
