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

# Regulatory watch list — is anything due for a re-read?
#
# Time-dependent, so it is not part of `just ci`: an overdue reading means a
# human must go and look at a publisher's website, which is a different kind of
# failure from a broken build. CI runs it as a job of its own.
watch:
    cargo test --all-features --test watch -- --include-ignored

# Full CI gate — run before every commit.
ci: fmt-check lint verify-data fuzz-seeds-check test-all test-no-features
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
#
# `fuzz/seeds/<target>` is passed when it exists. For `parse` it is load-bearing
# rather than an optimisation: random bytes are never a well-formed ISO 20022
# document, so an unseeded run never reaches the parsers' own logic.
fuzz TARGET="parse" SECS="60":
    #!/usr/bin/env bash
    set -euo pipefail
    # libFuzzer requires every corpus directory on the command line to exist,
    # and `fuzz/corpus/` is gitignored — it is generated, not source. Naming
    # it explicitly means cargo-fuzz does not create it for us.
    mkdir -p "fuzz/corpus/{{ TARGET }}"
    seeds=""
    [ -d "fuzz/seeds/{{ TARGET }}" ] && seeds="fuzz/seeds/{{ TARGET }}"
    cargo +nightly fuzz run {{ TARGET }} \
        fuzz/corpus/{{ TARGET }} $seeds -- -max_total_time={{ SECS }}

# Regenerate `fuzz/seeds/parse` from the document fixtures in `src/` and `tests/`.
fuzz-seeds:
    python3 scripts/extract-fuzz-seeds.py

# Fail if the checked-in seeds no longer match the fixtures (what CI runs).
fuzz-seeds-check:
    python3 scripts/extract-fuzz-seeds.py --check

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
