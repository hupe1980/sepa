# Pinned ISO 20022 schemas

These XSDs are used by `tests/integration.rs` to validate the XML this crate
generates. They are **test fixtures only** — nothing in `src/` reads them.

| File | Used for |
|---|---|
| `pain.001.001.09.xsd` | SCT + SCT Instant (current default) |
| `pain.001.001.03.xsd` | SCT, EPC version until Nov 2023 |
| `pain.001.003.03.xsd` | SCT, legacy DK V2.7 |
| `pain.008.001.08.xsd` | SDD CORE + B2B (current default) |
| `pain.008.001.02.xsd` | SDD, EPC version until Nov 2023 |
| `pain.008.003.02.xsd` | SDD, legacy DK V2.7 |
| `pain.007.001.09.xsd` | SDD reversal — the only version SEPA defines |
| `pain.002.001.10.xsd` | Status report + Verification of Payee (parser fixtures) |
| `pain.001.001.09_GBIC_5.xsd` | SCT — the DK's stricter validation subset |
| `pain.008.001.08_GBIC_5.xsd` | SDD — the DK's stricter validation subset |

The file name is derived from the schema variant's `message_id()`, so adding a
variant to `CreditTransferSchema::ALL` / `DirectDebitSchema::ALL` without adding
its XSD here makes the validation tests skip loudly rather than pass silently.

## Why they are vendored

Pinning known-good copies makes validation reproducible and lets CI fail on a
real regression rather than on a flaky download.

## Where to get authoritative copies

The **[ISO 20022 Message Archive][archive]** — *not* the current-version
catalogue, which serves only the newest definition of each message
(`pain.001.001.13`, `pain.008.001.12`, …). SEPA is pinned by its rulebooks to
older versions, so the archive is the right source. Pick the payments-initiation
maintenance release containing `pain.001.001.09`; it carries `pain.008.001.08`,
`pain.002.001.10` and `pain.007.001.09` too.

Copies obtained from the archive are authoritative and need no corroboration.
The entries below that cite third-party mirrors predate that route and were
cross-checked byte-for-byte between two independent sources instead.

[archive]: https://www.iso20022.org/catalogue-messages/iso-20022-messages-archive?search=pain

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

Two sources, and the difference matters.

### Official — Deutsche Kreditwirtschaft, via [ebics.de][ebics]

Freely published, no registration. These are authoritative.

| File | SHA-256 |
|---|---|
| `pain.007.001.09.xsd` | `fcd46ce9e3df24fb610f7dd122ceeae23ea86e14ff38ff830eafc172c5155b19` |
| `pain.001.001.09_GBIC_5.xsd` | `d64da6e1553cf8dd38f98818fe3234fb79daec792d0a69483a1ab8e90dec8b8c` |
| `pain.008.001.08_GBIC_5.xsd` | `f0d9429a114f496d78644a0b734452aa0e33ad4ae082fcbf806923fe138ad754` |
| `pain.002.001.10.xsd` | `3eaa417745a92d7077d6f966b5a35943014c92164d2973b671d16012bf733686` |

The `_GBIC_5` files and `pain.007.001.09.xsd` are **technical validation
subsets**: restrictions of the ISO schema down to what German banks accept, so
they are *stricter* than plain ISO. Passing them is the harder test — it is what
established that `OrgnlTxRef` and its mandate are mandatory on a reversal, where
ISO leaves both optional. `pain.002.001.10.xsd` is the unrestricted ISO original
that the DK bundles alongside; it is byte-identical in the GBIC 4 and GBIC 5
packages.

Nothing generates pain.002 — that schema exists to prove the parser's fixtures,
including the Verification of Payee report, are real documents rather than
invented shapes.

### Mirror-sourced — ISO originals

`iso20022.org` serves only the *newest* version of each message
(`pain.001.001.13`, …), and SEPA is pinned to older ones by its rulebooks. The
older ISO originals live in the [ISO 20022 Message Archive][archive]; where that
was not reachable, the files below came from third-party mirrors and were
cross-checked byte-for-byte between two independent copies.

- `pain.008.001.08.xsd` — SHA-256
  `3b2efe2239fceea22b17eb980df58c8b80db7322cf7781f8fd64ee8d17696210`.
  From [sepa.js](https://github.com/kewisch/sepa.js/blob/main/schema/pain.008.001.08.xsd),
  corroborated byte-for-byte (after CRLF→LF) against a copy attached to
  [php-sepa-xml#161](https://github.com/php-sepa-xml/php-sepa-xml/files/14664481/pain.008.001.08.xsd.zip).
  Generator stamp `Standards Editor (build:R1.6.15) on 2019 Feb 14`.
- `pain.001.001.09.xsd` — from
  [fortesp/xsd2xml](https://github.com/fortesp/xsd2xml/blob/master/tests/resources/pain.001.001.09.xsd).
- `pain.001.001.03.xsd` — SHA-256
  `ae2bbba02a6be0119a26f4afcb65ced067453cb1b81d38b26bcc569f19eca93e`.
- `pain.008.001.02.xsd` — SHA-256
  `09b13e91fcde87f3153a4a417c866008fbc3d8a91706c0b24ff2bf4a18b56429`.

  Both from [sepa.js](https://github.com/kewisch/sepa.js/tree/main/schema) and
  corroborated against
  [python-sepaxml](https://github.com/raphaelm/python-sepaxml/tree/master/sepaxml/schemas):
  identical after CRLF→LF and comment normalisation. Generator stamp
  `SWIFTStandards Workstation (build:R6.1.0.2) on 2009 Jan 08`, and their
  `ServiceLevel8Choice` / `LocalInstrument2Choice` / `AccountIdentification4Choice`
  really are `xs:choice` — see the warning above.
- `pain.001.003.03.xsd`, `pain.008.003.02.xsd` — from
  [willuhn/hbci4java](https://github.com/willuhn/hbci4java), the German DK
  schemas per DFÜ-Abkommen Anlage 3 V2.7.

Replacing a mirror-sourced file with an archive copy is a welcome change; the
sensible check afterwards is that the test suite still passes, since the DK
subsets validate the same documents from the other direction.

[ebics]: https://www.ebics.de/de/datenformate/ergaenzende-dokumente
[archive]: https://www.iso20022.org/catalogue-messages/iso-20022-messages-archive?search=pain

## Two layers of validation

Generated `pain.001.001.09` and `pain.008.001.08` documents are validated
against **both** the ISO schema and the DK subset. The ISO schema answers "is
this ISO 20022", the subset answers "would a German bank take it", and they
catch different things — the ISO schema permits a zero amount and a
141-character name; the subset makes elements mandatory that ISO does not.

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
