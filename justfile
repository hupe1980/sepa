# sepa — development task runner
# Install just: https://just.systems/man/en/

default:
    @just --list --unsorted

# Check formatting without making changes.
fmt-check:
    cargo fmt --all --check

# Format all source files.
fmt:
    cargo fmt --all

# Run Clippy on all targets and features (warnings are errors).
lint:
    RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features -- -D warnings

# Quick type-check (fastest feedback loop).
check:
    cargo check --all-targets --all-features

# Run unit + doc tests with default features.
test *ARGS:
    cargo test {{ ARGS }}

# Run tests with all features enabled.
test-all:
    RUSTFLAGS="-D warnings" cargo test --all-targets --all-features

# Run tests with no default features.
test-no-features:
    RUSTFLAGS="-D warnings" cargo test --all-targets --no-default-features

# Test against the declared MSRV (requires `rustup toolchain install 1.85`).
test-msrv:
    cargo +1.85 test --all-targets --all-features

# Run a specific test by name filter.
test-one FILTER:
    cargo test --all-features {{ FILTER }}

# Full CI gate — run before every commit.
ci: fmt-check lint test-all test-no-features
    @echo "CI gate passed."

# ── Examples ──────────────────────────────────────────────────────────────────

# Run all examples.
examples: example-sepa-batch

# Run the SEPA payment batch example.
example-sepa-batch:
    cargo run --example sepa_batch

# ── Documentation ─────────────────────────────────────────────────────────────

# Build and open documentation in the browser.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --open

# Build documentation without opening (useful in CI).
doc-build:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# ── Security ──────────────────────────────────────────────────────────────────

# Audit dependencies for known vulnerabilities (requires `cargo install cargo-audit`).
audit:
    cargo audit

# Check licenses and advisories (requires `cargo install cargo-deny`).
deny:
    cargo deny --all-features check

# ── Fuzzing ───────────────────────────────────────────────────────────────────

# Fuzz a target (requires nightly + `cargo install cargo-fuzz`).
# Targets: parse, identifiers, build
fuzz TARGET="parse" SECS="60":
    cargo +nightly fuzz run {{ TARGET }} -- -max_total_time={{ SECS }}

# ── Schema validation ─────────────────────────────────────────────────────────

# Validate generated XML against the pinned ISO 20022 XSDs (requires xmllint).
xsd:
    cargo test --all-features --test integration xsd:: -- --nocapture
