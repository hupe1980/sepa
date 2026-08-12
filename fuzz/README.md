# Fuzz targets

This crate parses files supplied by third parties over EBICS and FinTS. A panic
there is a denial of service on a payment pipeline, so the parsers, validators
and builders are fuzzed.

Three real panics have been found and fixed this way, all from byte-indexing
text that arrived from outside:

- `&desc[..140]` sliced a remittance description by byte index and crashed on
  any multi-byte character near the limit.
- `ct_from_eur_str` sliced the fractional part by byte index, so an amount such
  as `<Amt Ccy="EUR">1.&#8364;5</Amt>` panicked once the XML layer decoded the
  entity to `€`.
- `IsoDateTime::parse` split the UTC offset off at `len - 6`, which for
  `"2026-07-20T€€a"` lands inside the first `€`.

## Running

Requires a nightly toolchain:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run parse
cargo +nightly fuzz run identifiers
cargo +nightly fuzz run build_batch
```

## Targets

| Target | Covers | Invariant |
|---|---|---|
| `parse` | pain.002 (incl. Verification of Payee), camt.052/053/054 and the result accessors | never panics; `Ok` or `Err` only |
| `identifiers` | IBAN, BIC, Creditor ID, RF reference, amounts, dates, addresses, transliteration | never panics; **transliteration output is always SEPA-legal** |
| `build_batch` | the pain.001, pain.008 and pain.007 builders end to end | never panics; any accepted batch is a complete document |

The `identifiers` and `build_batch` targets assert invariants rather than only
checking for crashes, so they fail on silent corruption too.

## Corpus

Seed from the test fixtures for faster coverage:

```sh
mkdir -p corpus/parse
cargo test --all-features            # writes nothing, but see tests/integration.rs
# then add real (anonymised) bank files to corpus/parse/
```
