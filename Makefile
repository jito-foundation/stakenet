# Makefile for Stakenet
.PHONY: check build build-release build-sbf build-idl test

# Check the project
check:
	cargo check --features idl-build

# Build the project
build:
	cargo build --features idl-build

# Build the project
build-release:
	cargo build --release --features idl-build

build-sbf:
	cargo-build-sbf --manifest-path programs/steward/Cargo.toml
	cargo-build-sbf --manifest-path programs/validator-history/Cargo.toml

# IDL Build
build-idl:
	anchor idl build -p steward -o ./programs/steward/idl/steward.json
	anchor idl build -p validator-history -o ./programs/validator-history/idl/validator_history.json

# Run tests
test:
	./run_tests.sh
