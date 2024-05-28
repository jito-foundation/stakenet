#!/bin/bash

# Build programs
cargo build-sbf --manifest-path programs/steward/Cargo.toml;
cargo build-sbf --manifest-path programs/validator-history/Cargo.toml;

# Run all tests except the specified one
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=5000000 cargo test -- --skip steward::test_state_methods

# Check if the previous command succeeded
if [ $? -eq 0 ]; then
    # Run the specific test in isolation
    SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=5000000 cargo test --package tests --test mod steward::test_state_methods
else
    echo "Some tests failed, skipping the isolated test run."
fi