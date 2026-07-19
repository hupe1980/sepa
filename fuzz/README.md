# Fuzz targets

This crate parses files supplied by third parties over EBICS and FinTS. A panic
there is a denial of service on a payment pipeline, so the parsers, validators
and builders are fuzzed.

Two real panics were found and fixed this way — both reachable from a bank file:

- `&desc[..140]` sliced a remittance description by byte index and crashed on
  any multi-byte character near the limit.
- `ct_from_eur_str` sliced the fractional part by byte index, so an amount such
  as `<Amt Ccy="EUR">1.&#8364;5</Amt>` panicked once the XML layer decoded the
  entity to `€`.

## Running

Requires a nightly toolchain:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse
cargo +nightly fuzz run identifiers
cargo +nightly fuzz run build
```

## Targets

| Target | Covers | Invariant |
|---|---|---|
| `parse` | pain.002, camt.052/053/054 and the result accessors | never panics; `Ok` or `Err` only |
| `identifiers` | IBAN, BIC, Creditor ID, RF reference, amounts, transliteration | never panics; **transliteration output is always SEPA-legal** |
| `build` | both builders end to end | never panics; any accepted batch is a complete document |

The `identifiers` and `build` targets assert invariants rather than only
checking for crashes, so they fail on silent corruption too.

## Corpus

Seed from the test fixtures for faster coverage:

```sh
mkdir -p corpus/parse
cargo test --all-features            # writes nothing, but see tests/integration.rs
# then add real (anonymised) bank files to corpus/parse/
```
