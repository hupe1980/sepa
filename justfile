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

# Test against the declared MSRV (requires `rustup toolchain install 1.88`).
test-msrv:
    cargo +1.88 test --all-targets --all-features

# Run a specific test by name filter.
test-one FILTER:
    cargo test --all-features {{ FILTER }}

# Full CI gate — run before every commit.
ci: fmt-check lint verify-data test-all test-no-features
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
# Targets: parse, identifiers, build_batch
fuzz TARGET="parse" SECS="60":
    cargo +nightly fuzz run {{ TARGET }} -- -max_total_time={{ SECS }}

# Fuzz every target in turn — what CI runs on each push.
fuzz-all SECS="60":
    just fuzz parse {{ SECS }}
    just fuzz identifiers {{ SECS }}
    just fuzz build_batch {{ SECS }}

# ── Site ──────────────────────────────────────────────────────────────────────

# Serve the documentation site locally (requires `zola`).
site-serve:
    cd site && zola serve

# Build the documentation site into site/public.
site-build:
    cd site && zola build

# Check the site's internal links and anchors.
site-check:
    cd site && zola check

# ── Vendored reference data ───────────────────────────────────────────────────

# Verify the pinned XSDs against the digests in tests/xsd/README.md.
verify-data:
    ./scripts/check-vendored-data.sh

# Re-check every vendored table against the examples its publisher ships.
# A failure here means the data no longer matches its source, not that a test
# is flaky — see tests/xsd/README.md for provenance and what to re-fetch.
verify-tables: verify-data
    cargo test --all-features --lib -- \
        iban::tests::every_published_registry_example_validates \
        iban::tests::every_registry_example_is_reproduced_from_its_own_bban \
        iban::tests::registry_has_full_swift_entry_count \
        iban::tests::registry_structures_are_well_formed \
        country::tests::every_iban_registry_country_is_a_country_code \
        country::tests::every_sepa_country_is_a_country_code \
        charset::tests::no_table_entry_maps_a_character_that_is_already_legal \
        charset::tests::every_replacement_is_itself_legal_and_non_empty \
        charset::tests::exactly_twenty_entries_lengthen_the_text \
        --exact

# ── Schema validation ─────────────────────────────────────────────────────────

# Validate generated XML against the pinned ISO 20022 XSDs (requires xmllint).
xsd:
    cargo test --all-features --test integration xsd:: -- --nocapture
