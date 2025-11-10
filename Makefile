# Makefile for Stakenet
.PHONY: check build build-release test

# Check the project
check:
	cargo check --features idl-build

# Build the project
build:
	cargo build --features idl-build

# Build the project
build-release:
	cargo build --release --features idl-build

# Run tests
test:
	./run_tests.sh
