#!/bin/bash
set -e

# Build programs
 ~/.cargo/bin/solana-verify build --library-name jito_steward -- --features mainnet-beta
 ~/.cargo/bin/solana-verify build --library-name validator_history

# Run all tests
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p tests --all-features --color auto
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p stakenet-sdk  --all-features --color auto
SBF_OUT_DIR=$(pwd)/target/deploy RUST_MIN_STACK=20000000 cargo nextest run -p jito-steward --all-features --color auto
