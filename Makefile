# Makefile for Stakenet
.PHONY: check build test

# Check the project
check:
	cargo check --features idl-build

# Build the project
build:
	cargo build --features idl-build

# Run tests
test:
	./run_tests.sh
