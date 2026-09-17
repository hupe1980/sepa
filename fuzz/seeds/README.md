# Fuzz seed corpora

Checked-in starting inputs for `cargo fuzz`, one directory per target.

## Why these exist, for `parse` especially

Random bytes are never a well-formed ISO 20022 document. An unseeded run of the
`parse` target therefore spends its whole budget on the XML rejection path and
never reaches the parsers' own logic — which is measurable rather than
theoretical:

| Run | Result |
|---|---|
| 315,449 executions, no seeds, sign-flip defect present | **not found** |
| seeded with these documents, same defect present | **found in seconds** |

That is why the seeds are load-bearing rather than an optimisation, and why
they are checked in rather than left to a warm CI cache. The `parse` target
asserts that no figure is reported which the input did not carry; an assertion
the input distribution cannot reach is not a gate.

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
