# Fuzz seed corpora

Checked-in starting inputs for `cargo fuzz`, one directory per target.

Parser seeds help mutations reach message interpretation rather than only XML
rejection. They complement hostile bytes and do not prove complete coverage.

## Provenance

Extracted from the document literals in `tests/` and `src/` — the same fixtures
the parser tests and the `xmllint` schema gate share, so a fixture that gains
coverage in one gains it in all three. Regenerate after adding a fixture:

```sh
just fuzz-seeds
```

`build_batch` and `identifiers` take structured input (`(&str, &str, i64, u8)`
and arbitrary strings), which libFuzzer generates usefully on its own, so
neither has a seed directory.
