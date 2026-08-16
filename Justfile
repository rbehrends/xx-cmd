# Common development commands for xx.

set shell := ["sh", "-cu"]

[private]
list:
	just --list

# Show the available recipes.
default:
    just --list

# Build the debug binary.
build:
    cargo build --locked

# Build an optimized release binary.
release:
    cargo build --release --locked


# Type-check the project without producing a release binary.
check:
    cargo check --locked


# Run the test suite, including integration tests.
test:
    cargo test --locked

# Run only the process-level command-line integration tests.
test-cli:
    cargo test --test cli --locked

# Regenerate externally-produced integration fixtures.
regenerate-fixtures:
    ./scripts/regenerate-fixtures.sh


# Format the Rust sources.
format:
    cargo fmt --all

# Verify that the Rust sources are formatted.
format-check:
    cargo fmt --all -- --check

# Run Clippy with warnings treated as errors.
clippy:
    cargo clippy --all-targets --all-features --locked -- -D warnings


# Run formatting and lint checks.
lint: format-check clippy

# Run the complete local verification suite.
verify: format-check check test clippy


# Build API documentation.
doc:
    cargo doc --no-deps --locked

# Run xx with command-line arguments, for example: just run --list archive.tar
run *args:
    cargo run --locked -- {{args}}
