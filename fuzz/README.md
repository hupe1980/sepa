# Fuzz targets

The targets exercise untrusted text, XML, amounts and nested payment inputs.
They check panic safety and selected semantic invariants, not complete coverage.

## Running

Run from the repository root with a nightly toolchain:

```sh
cargo install cargo-fuzz
mkdir -p fuzz/corpus/parse
cargo +nightly fuzz run parse fuzz/corpus/parse fuzz/seeds/parse
cargo +nightly fuzz run identifiers
cargo +nightly fuzz run build_batch
```

## Targets

| Target | Covers | Invariant |
|---|---|---|
| `parse` | pain.002, camt.029, camt.052/053/054, limited intake and result accessors | panic safety; selected money/direction and aggregation invariants |
| `identifiers` | IBAN, BIC, Creditor ID, RF reference, amounts, dates, addresses, transliteration | never panics; **transliteration output is always SEPA-legal** |
| `build_batch` | pain.001, pain.008, pain.007 and camt.055 builders | panic safety; accepted batches have a document envelope and SEPA-legal text |

## Corpus

The first directory is writable; generated seeds are input-only in the command above.

Regenerate the parser seeds after changing test fixtures:

```sh
just fuzz-seeds
```

Use anonymised, authorised bank files for additional corpus inputs. Schema checks
belong to the integration/fixture suites; fuzzing does not establish XSD validity.
