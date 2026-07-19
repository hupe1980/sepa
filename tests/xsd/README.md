# Pinned ISO 20022 schemas

These XSDs are used by `tests/integration.rs` to validate the XML this crate
generates. They are **test fixtures only** — nothing in `src/` reads them.

| File | Used for |
|---|---|
| `pain.001.001.09.xsd` | SCT + SCT Instant (current default) |
| `pain.001.003.03.xsd` | SCT, legacy DK V2.7 |
| `pain.008.001.08.xsd` | SDD CORE + B2B (current default) |
| `pain.008.003.02.xsd` | SDD, legacy DK V2.7 |

## Why they are vendored

ISO 20022 gates schema downloads behind a registration wall, and the mirrors
that do publish them are not uniformly trustworthy — see the warning below.
Pinning known-good copies makes the validation reproducible and lets CI fail on
a real regression rather than on a flaky download.

## ⚠️ Not every mirror is correct

A widely-mirrored copy of `pain.008.001.08.xsd` (served from `partners.lhv.ee`)
is **defective**: it flattens `xs:choice` into `xs:sequence`, which makes
`Prtry` mandatory inside `ServiceLevel8Choice`. A *correct*
`<SvcLvl><Cd>SEPA</Cd></SvcLvl>` fails against it with:

```
element SvcLvl: Schemas validity error : Element '...SvcLvl':
Missing child element(s). Expected is ( ...Prtry ).
```

If you ever replace these files, verify that the choice types
(`ServiceLevel8Choice`, `LocalInstrument2Choice`, `Party38Choice`,
`AccountIdentification4Choice`) really use `<xs:choice>` before trusting them.

## Provenance

- `pain.008.001.08.xsd` — SHA-256
  `3b2efe2239fceea22b17eb980df58c8b80db7322cf7781f8fd64ee8d17696210`.
  Obtained from [sepa.js](https://github.com/kewisch/sepa.js/blob/main/schema/pain.008.001.08.xsd)
  and independently corroborated byte-for-byte (after CRLF→LF normalisation)
  against a copy attached to
  [php-sepa-xml#161](https://github.com/php-sepa-xml/php-sepa-xml/files/14664481/pain.008.001.08.xsd.zip).
  Carries the generator stamp `Standards Editor (build:R1.6.15) on 2019 Feb 14`
  — the same ISO 2019 maintenance release batch as `pain.001.001.09.xsd`.
- `pain.001.001.09.xsd` — from
  [fortesp/xsd2xml](https://github.com/fortesp/xsd2xml/blob/master/tests/resources/pain.001.001.09.xsd).
- `pain.001.003.03.xsd`, `pain.008.003.02.xsd` — from
  [willuhn/hbci4java](https://github.com/willuhn/hbci4java), the German DK
  schemas per DFÜ-Abkommen Anlage 3 V2.7.

## Running the checks

```sh
just xsd
# or
cargo test --all-features --test integration xsd:: -- --nocapture
```

Requires `xmllint` (`libxml2-utils` on Debian/Ubuntu; preinstalled on macOS).
Without it the tests print `SKIP:` and pass, so a missing tool never masquerades
as a green validation — CI installs it to make sure the checks actually execute.

## Schema validation is necessary, not sufficient

The ISO schemas are far more permissive than the banks. All of the following
validate cleanly and are still rejected on ingestion:

- `<InstdAmt Ccy="EUR">0</InstdAmt>` — the generic type allows zero and five
  decimal places
- `<BICFI>NOTPROVIDED</BICFI>` — the literal happens to satisfy the BIC pattern
- a 140-character `<Nm>` — the EPC limit is 70
- a batch with no `CdtrSchmeId` — mandatory per the SDD rulebooks, optional in
  the XSD

Those rules live in `src/validate.rs` and are enforced by `build()`.
