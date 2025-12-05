# Makefile for Stakenet
.PHONY: check build build-release idl-build test

# Check the project
check:
	cargo check --features idl-build

# Build the project
build:
	cargo build --features idl-build

# Build the project
build-release:
	cargo build --release --features idl-build

# IDL Build
idl-build:
	anchor idl build -p steward -o ./programs/steward/idl/steward.json
	anchor idl build -p validator-history -o ./programs/validator-history/idl/validator_history.json

# Run tests
test:
	./run_tests.sh
