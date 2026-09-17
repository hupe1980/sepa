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
| `pain.002.001.03.xsd` | Status report, ISO pre-2023 generation (parser fixtures) |
| `pain.002.003.03.xsd` | Status report, legacy DK V2.7 (parser fixtures) |
| `pain.001.001.09_GBIC_5.xsd` | SCT — the DK's stricter validation subset |
| `pain.008.001.08_GBIC_5.xsd` | SDD — the DK's stricter validation subset |
| `camt.055.001.05.xsd` | Payment cancellation request (recall) |
| `camt.029.001.06.xsd` | Resolution of investigation — the answer to a recall (parser fixtures) |
| `camt.052.001.08.xsd` | Intraday report (parser fixtures) |
| `camt.053.001.08.xsd` | End-of-day statement (parser fixtures) |
| `camt.053.001.06.xsd` | End-of-day statement, 2016 version (parser fixtures) |
| `camt.054.001.08.xsd` | Debit/credit notification (parser fixtures) |

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
invented shapes. The same argument covers the four read-only camt schemas: a
hand-built fixture that no schema has seen proves only that the parser agrees
with whoever wrote it.

### Official — Deutsche Kreditwirtschaft, camt bundles

Distributed with the DFÜ-Abkommen Anlage 3 packages alongside the pain schemas
above, and authoritative for the same reason.

| File | SHA-256 |
|---|---|
| `camt.055.001.05.xsd` | `6e18c49a4ff81023d18d9dc499e514697114616671cc462202fd859cefa4eae8` |
| `camt.029.001.06.xsd` | `39e2209ea36910875076ad7e20bbcc796ba4ae3d0e45bccb311db4e1bf0e848f` |
| `camt.052.001.08.xsd` | `9bb093a0c39cd278d25b40d9ec8c59d1e8f6ab37536c83b84d41bbf5c7f8bd6c` |
| `camt.053.001.08.xsd` | `338e9cb0c9989b5181802a7b773eece070d6815fc9d6483ac0579117bc24ccba` |
| `camt.054.001.08.xsd` | `13d220337d47e22cf25788807c136794a76955791df612c6947277272d440da6` |

`camt.055.001.05` and `camt.029.001.06` are the pair the DFÜ-Abkommen names for
a customer recall and its answer; unlike the payment-initiation messages there
is no per-bank version choice. Note that both keep the **pre-2019 BIC pattern**
under an element named `BICFI` — see `src/bic.rs`, where that is why the
character pattern and the element name are read from the schema separately.

The three camt.05x schemas are the 2019 versions. They gate the parser fixtures
rather than any generated output: nothing in this crate writes camt.05x.

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
- `pain.001.001.09.xsd` — SHA-256
  `de038b373e47b0077b1832ddd81f4b2f1eb25d35721f62da1e38b7f5a09fda24`. From
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
- `pain.001.003.03.xsd` — SHA-256
  `58c51f123ac66c4981adfede7478851cfc76c6a5e722f7cbff7e2b9719aa22fc`.
- `pain.008.003.02.xsd` — SHA-256
  `2bfaaae0239adef8d62ddf8c48472aec640971724425a26e493fccca586e805a`.
- `pain.002.003.03.xsd` — SHA-256
  `9d7d07c228e180ebc4a68245fa6fa7e4907025dcbbeccffcfdcb1ea3a5521130`.

  All three from [willuhn/hbci4java](https://github.com/willuhn/hbci4java), the
  German DK schemas per DFÜ-Abkommen Anlage 3 V2.7.

  `pain.002.003.03` is **single-sourced**: the format was retired with DFÜ V2.7
  in November 2022 and no second copy exists to diff against. The
  accept/reject pair below stands in for the byte-comparison.
- `pain.002.001.03.xsd` — SHA-256
  `c0c2b98f51638147598678bbec3d5aedfa270a819c2fda8014ba3c34a5972195`. From
  [sladjan/xsd-camt](https://github.com/sladjan/xsd-camt), corroborated
  byte-for-byte against
  [sebastienrousseau/pain001](https://github.com/sebastienrousseau/pain001).
  Generator stamp `SWIFTStandards Workstation (build:R6.1.0.2) on 2009 Jan 08`.

  ⚠️ hbci4java *also* ships a `pain.002.001.03.xsd`, and it is **not** this
  one — it is the DK's restricted Bank-Kunde-Bank schema under the ISO
  namespace. Not defective, just a different artefact with the same name, and
  vendoring it would silently narrow the gate.
- `camt.053.001.06.xsd` — SHA-256
  `f09fcac3f524a231fc06bdc0da4014a1054b5aeb1797de591daeee7c2d6d6242`. From
  [sebastienrousseau/camt053](https://github.com/sebastienrousseau/camt053),
  corroborated byte-for-byte (after CRLF→LF) against
  [jHetzer/go-camt](https://github.com/jHetzer/go-camt). Generator stamp
  `Standards Editor (build:R1.6.5.6) on 2016 Feb 12`. Real `<xs:choice>` in
  `AccountIdentification4Choice` and `ChargeType3Choice`, checked.

Replacing a mirror-sourced file with an archive copy is a welcome change; the
sensible check afterwards is that the test suite still passes, since the DK
subsets validate the same documents from the other direction.

### Every schema is checked to reject, not only to accept

A schema stripped of its restrictions accepts every fixture too, and two
mirrors can share a defective ancestor — so agreement is not proof. Each
vendored schema must also **refuse** a document breaking a rule only it
enforces (`integration::xsd::each_vendored_schema_rejects_what_only_it_forbids`):

| Schema | Must refuse |
|---|---|
| `pain.002.003.03` | `GrpSts` = `ACTC` — the DK variant is reject-only |
| `camt.053.001.06` | a statement without `Stmt/CreDtTm`, mandatory only here |
| `pain.002.001.03` | a group without `OrgnlMsgNmId` |

For the single-sourced schema that pair of checks is the whole bound.

## The digests are enforced, not decorative

`scripts/check-vendored-data.sh` re-derives every SHA-256 above and fails on a
mismatch — and, just as importantly, fails when a `.xsd` in this directory has
**no** recorded digest. A schema silently swapped for a defective mirror is the
failure mode that makes correct output look wrong (see the warning above), and
the natural reaction to it is to "fix" the writer, at which point the crate
emits genuinely invalid files with a green suite. CI runs the script on every
push; `just verify-data` runs it locally.

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
