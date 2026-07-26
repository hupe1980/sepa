# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Versioning policy

This crate tracks a moving regulatory target — SEPA rulebooks and ISO 20022
message versions change on scheduled cutovers — so a version bump can change
what a bank receives, not only what the compiler accepts. Every release entry
therefore calls out, explicitly and near the top:

- **Emitted output** — any change to the schema version, element shape or
  default of generated XML. These are the changes that need a re-test against
  your bank, even when your code still compiles.
- **API** — any change to a public signature or error type.

While the crate is `0.x`:

- A **minor** bump (`0.4 → 0.5`) may contain breaking API changes and changes to
  emitted output. Read this file before taking one.
- A **patch** bump (`0.5.0 → 0.5.1`) never changes a public signature and never
  changes emitted output for an input that was previously accepted. It is
  reserved for fixes to input that was previously *rejected* or *mis-parsed*,
  and for documentation.
- A change to a **default schema version** is always a minor bump with a
  migration note, never a patch.
- A rise in the **minimum supported Rust version** is a minor bump, called out
  under its own heading.

Pin `sepa = "0.5"` and treat a move to `0.6` as a deliberate migration.

## [0.5.0]

An API, validation and coverage release, driven by production feedback from a
customer-account ledger using the crate for direct-debit collection runs and
bank reconciliation. **Breaking API changes with no deprecation path.** Two
schema-correctness defects and several smaller ones are fixed along the way.

### Changed — emitted output

- **The `SMNDA` amendment marker moves to its pre-2016 position under
  `pain.008.003.02`.** The DK schema's `OrgnlDbtrAcct/Id` admits nothing but an
  `IBAN`, and enumerates `SMNDA` as a value of
  `OrgnlDbtrAgt/FinInstnId/Othr/Id` instead — so every "same mandate, new debtor
  account" collection built against that schema was **schema-invalid**. The ISO
  schemas keep the post-2016 `OrgnlDbtrAcct` placement, which is unchanged.
- **SCT Instant on `pain.001.003.03` is now rejected instead of emitted.** That
  schema's `PmtTpInf` has no `LclInstrm` element at all, so the builder was
  writing one the XSD does not allow. It now fails with
  `ValidationError::UnsupportedBySchema`. The type-level documentation claimed
  the schema was switched to `pain.001.001.09` automatically; nothing did that,
  and nothing does now — the combination is an error, not a silent rewrite.
- No other change to the bytes emitted for an input that was accepted before.

### Added — validation

- **IBAN structure is checked, not just the checksum.** Mod-97 detects an
  altered character with probability 96/97 and never says which one, so an `O`
  typed for a `0` in a German account number passes it about 99% of the time it
  is the only error. The SWIFT IBAN Registry's BBAN structure for all 89
  countries is now enforced per character, reporting the position, the class the
  registry requires and the country's structure string — see
  [`iban_bban_format`] and [`BbanCharClass`]. The registry table is now the
  single source of truth: `iban_country_length` is derived from it, so length
  and structure cannot disagree. Vendored from SWIFT IBAN Registry release 101
  and checked in CI against the 78 real IBAN examples the registry publishes.
- **The ISO 13616 IBAN header is checked.** Characters 1–2 must be letters and
  3–4 digits; mod-97 expands a digit in the country code without complaint.
  New `IbanError::InvalidCountryCode` and `IbanError::NonNumericCheckDigits`.
- **BIC country codes must be countries.** The SEPA pattern accepts `COBAZZFF`,
  which addresses nothing. Characters 5–6 are now checked against the 249
  officially assigned ISO 3166-1 alpha-2 codes plus `XK`, which SWIFT issues
  Kosovan BICs under. New `BicError::UnknownCountryCode` and
  `is_bic_country_code`. A test asserts every registered IBAN country is also an
  acceptable BIC country, so the two tables cannot drift apart.

### Added

- **Selectable schema versions.** `pain.001.001.03` and `pain.008.001.02` — the
  EPC versions in force until 19 November 2023, and still what a number of banks
  and corporate channels accept or require — join the existing current and DK
  variants:

  | Message | Versions |
  |---|---|
  | pain.001 | `pain.001.001.09` (default) · `pain.001.001.03` · `pain.001.003.03` |
  | pain.008 | `pain.008.001.08` (default) · `pain.008.001.02` · `pain.008.003.02` |

  Both enums gain `ALL`, `message_id()`, `Display` and `FromStr` (accepting the
  message identifier or the namespace URN), so a bank's required version can be
  read from configuration rather than compiled in. Each version is validated
  against its own vendored XSD in CI.
- **Typed dates.** New [`IsoDate`] and [`IsoDateTime`] types in the `date`
  module, validated at construction, with `Ord`, calendar arithmetic
  (`plus_days`, `epoch_days`), `serde` support and — behind the new `time` and
  `chrono` features — conversions in both directions. `time` and `chrono` are
  conversion-only: the crate's own calendar arithmetic stays dependency-free.
- **Located build errors.** `build()` and `validate()` now return `BuildError`,
  which pairs the existing typed `ValidationError` with a `Location` naming the
  group and transaction index. A rejected ten-thousand-row collection run now
  points at the row to fix: `PmtInf[1]/Tx[4711]: Dbtr/Nm is 74 characters …`.
- **Resolved per-transaction amounts for batch-booked camt entries.**
  `EntryDetail` gains `signed_ct()`, resolved by the parser from `TxDtls/Amt`,
  then `TxDtls/AmtDtls/TxAmt/Amt`, then — for a single-detail entry only — the
  entry total, and signed by `TxDtls/CdtDbtInd` where the detail carries its
  own. It is `None` when the statement does not determine the amount, which is
  precisely when the obvious fallback (reuse the entry total per detail)
  multiplies a batch by its transaction count. `EntryDetail` also gains
  `currency`, `indicator` and `is_return()`; a detail in a currency other than
  the entry's resolves to `None` rather than being summed against it.
  `CashEntry` gains `details_signed_sum_ct()` and `details_reconcile()`.
- **Typed dates on the read path too.** `IsoDate::parse_date_part` reads the
  date out of either form of an ISO 20022 `DateAndDateTimeChoice`, and
  `CashEntry::booking_date()` / `value_date()`, `StatementBalance::date()` and
  `Camt054Entry::value_date()` / `booking_date()` return `Option<IsoDate>`. The
  text a bank actually sent is kept alongside, in the `*_raw` fields, so a
  non-conforming file stays readable rather than being rejected.
- `DirectDebitGroup::requested_collection_date()` and
  `CreditTransferGroup::requested_execution_date()`.
- `Camt054Entry` now derives `PartialEq`/`Eq`; `CreditDebitIndicator` derives
  `Default` (`Credit`, matching how the parsers treat an absent `CdtDbtInd`).

### Changed — MSRV

- **The minimum supported Rust version rises from 1.85 to 1.88.**
  RUSTSEC-2026-0009 is a stack-exhaustion denial of service in `time`'s RFC
  2822 parser, fixed in `time` 0.3.47 — which requires Rust 1.88. This crate
  never calls that parser, so it was not itself exposed, but the dependency
  requirement pulled a flagged version into every downstream tree and failed
  their audits too. Raised for all builds rather than only for the `time`
  feature, so the MSRV stays one unconditional promise. `chrono` is unaffected.

### Changed — API

- **Dates are `IsoDate`, not `String`.** `DirectDebitGroup::collection_date`,
  `CreditTransferGroup::execution_date` and the `mandate_signed_at` argument of
  `DirectDebitEntry::new` take an `IsoDate`; `created_at` takes an
  `IsoDateTime`. This follows what the crate already did for `Iban`, `Bic` and
  `CreditorId`: validate once at construction, so a malformed date is
  unrepresentable in a batch rather than something `build()` has to catch.
  Migrate with `"2026-07-20".parse()?` or `IsoDate::new(2026, 7, 20)?`, or
  convert straight from `time::Date` / `chrono::NaiveDate`.
- **`DirectDebitGroup::new` borrows the Creditor Identifier** (`&CreditorId`),
  matching the `&Iban` beside it. Building one group per sequence type no longer
  clones the same creditor identity per group.
- **`build()` / `validate()` return `BuildError`** rather than
  `ValidationError`; the rule is still matchable as `err.kind`.
  `WriteError::Validation` wraps `BuildError`.
- **`ct_from_eur_str` returns `Result<i64, AmountError>`** rather than
  `Option<i64>`, distinguishing empty, malformed and overflowing input.
- **`camt054::parse_simple_json` returns `Result<_, SimpleJsonError>`** rather
  than `Option`. Every rejection names the field and the reason, so a bank row
  skipped during import can be logged with a diagnosable cause instead of
  disappearing. `date` is now required and validated, and optional fields are
  type-checked; `end_to_end_id` is now read if present.
- **camt date fields are renamed to `*_raw`.** `CashEntry::booking_date` /
  `value_date`, `StatementBalance::date` and `Camt054Entry::value_date` /
  `booking_date` are now `booking_date_raw` and so on, with the old names taken
  by the typed accessors above. Replace a field read with a call, or read the
  `_raw` field to keep the string.
- `ValidationError::InvalidDate` and `ValidationError::MissingCreditorId` are
  removed — the first is unreachable now dates are typed, the second was already
  unreachable since the Creditor Identifier became mandatory in
  `DirectDebitGroup::new`. `ValidationError::UnsupportedBySchema` is added.
- `check_date` is removed from the `validate` module; use `IsoDate::parse`.
- `Camt054Entry` gains `end_to_end_id` from `parse_simple_json`, which
  previously ignored the field.

### Fixed

- **A signed fractional part silently changed an amount.** `i64::from_str`
  accepts a leading sign, so `ct_from_eur_str("1.-5")` parsed its fraction as
  −5 cents and returned 0.95 EUR for what looked like 1.50 EUR. Reachable from
  every camt and pain.002 parser. Such amounts are now rejected.
- **`IsoDate::from_epoch_days` could overflow.** Day counts near `i64::MAX` were
  shifted before being range-checked. They are now rejected up front, and
  `plus_days` saturates into that check rather than overflowing.
- **The `build` fuzz target did not compile, and its crate would not build at
  all.** It still used the pre-0.4 single-group API, and cargo forbids a binary
  target named `build` — which made the whole `fuzz` manifest unparseable, so
  none of the three targets could run. The target is renamed `build_batch` and
  updated; it now also sweeps every schema version.
- **An empty purpose code was reported as too long.** `Purpose::Other("")`
  failed with "0 characters, exceeding the maximum of 4", because the shared
  length check folds "too short" and "too long" into one variant. It is now
  `ValidationError::Empty`.
- **Lengths in identifier errors were counted in bytes.** `IbanError`,
  `BicError`, `CreditorIdError` and `RfReferenceError` reported the UTF-8 byte
  count for input containing a non-ASCII character — which the very next check
  rejects anyway, but not before the wrong number reached a log. All are now
  counted in characters, as the length limits themselves are.

### Testing

- XSD validation now runs over **every** schema version rather than a fixed
  four, with dedicated cases for the agent-less "IBAN only" form, the `SMNDA`
  marker and SCT Instant in each version — the three places the versions differ
  structurally.
- New unit tests cover batch-booked amount resolution, including the
  double-counting case, per-detail `CdtDbtInd` overrides, `AmtDtls/TxAmt`
  fallback and foreign-currency details.
- The IBAN registry is checked against the 78 real IBAN examples the SWIFT
  registry publishes, structure and checksum both, so a transcription slip in
  89 hand-entered rows fails the build rather than a payment.
- The fuzz targets sweep the new surface: `IsoDate::parse_date_part` over the
  byte boundary a multi-byte character can straddle, the schema-version matrix,
  and the invariant that a validated IBAN always satisfies its own registered
  structure.

## [0.4.0]

A correctness and completeness release. Several defects meant the crate could
emit files that banks reject, and reject identifiers that banks accept.
**This release contains breaking API changes with no deprecation path.**

### Fixed

- **SEPA Creditor Identifier check digits were computed wrongly.** The mod-97
  calculation folded in the 3-character Creditor Business Code, which
  EPC262-08 explicitly excludes. The canonical `DE98ZZZ09999999999` was
  rejected while the invalid `DE74ZZZ09999999999` was accepted — that is,
  *every* genuine Creditor Identifier failed validation. The check now runs over
  the national identifier only, as `98 − (n mod 97)`.
- **Latvia was missing from the IBAN registry**, so every `LV` IBAN failed
  validation. Six further registry countries were also absent (`BI`, `DJ`, `FK`,
  `HN`, `OM`, `RU`). The table now holds all 89 registry entries, with a test
  that fails if the count drifts.
- **SCT Instant files never validated against their own schema.** Setting
  `LocalInstrument::Inst` switches to `pain.001.001.09`, where `ReqdExctnDt` is
  a `DateAndDateTime2Choice` requiring a `<Dt>` child. The builder emitted a
  bare date, so every generated SCT Inst file was schema-invalid. Now emitted as
  `<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>` for `IsoV9`, and as a bare
  date for the DK schema, which takes an `ISODate`.
- **`NOTPROVIDED` was written as a BIC.** `<BIC>NOTPROVIDED</BIC>` happens to
  satisfy the BIC pattern, so it passes XSD validation and is then rejected at
  bank ingestion. Agents with no known BIC now use the EPC "IBAN only" form,
  `<Othr><Id>NOTPROVIDED</Id></Othr>`. For credit transfers, `CdtrAgt` is
  omitted entirely, as the EPC guidelines require.
- **A remittance description could panic the process.** `&desc[..140]` sliced by
  byte index, so any text whose 140th byte fell inside a multi-byte character —
  routine for German remittance lines — panicked. Lengths are now measured and
  truncated in characters, which is also what ISO 20022 specifies.
- **A near-maximum `MsgId` produced a schema-invalid file.** `PmtInfId` was
  derived as `MsgId + "-1"`, so a 35-character `MsgId` — itself perfectly legal
  — yielded a 37-character `PmtInfId` that breached the `Max35Text` facet.
  `PmtInfId` now defaults to the `MsgId` unchanged and is validated in its own
  right; set it explicitly with `payment_info_id(…)`.
- **`ct_from_eur_str` could panic on a bank-supplied amount.** The fractional
  part was sliced by byte index, so `<Amt Ccy="EUR">1.&#8364;5</Amt>` — which
  decodes to `1.€5` — panicked the process. Reachable through the public
  `parse_camt053`. Such amounts are now rejected as unparseable.
- **The XML parser accepted documents with more than one root element**, and the
  *last* one won. Appending a second `<Document>…</Document>` to a file was a
  document-substitution primitive: this library read different data than a
  validator or the next consumer saw. Multiple roots and unmatched end tags are
  now rejected as malformed.
- **The XML parser mis-read valid documents.** The hand-rolled string scanner
  treated commented-out elements as real (a `<!-- <MsgId>…</MsgId> -->` was
  returned as the message ID), never decoded entity references (`&amp;` stayed
  literal), ignored CDATA, and mis-scanned self-closing tags. Parsing is now
  built on [`quick-xml`](https://crates.io/crates/quick-xml).
- **BIC test-code detection used the wrong position.** A test BIC is marked by
  the *second* character of the location code (position 8), not the first.
  `DEUTDE0B` was reported as a test BIC; it is not one — and under the SEPA
  pattern it is not a valid BIC at all.
- **camt.053 entry status broke on modern statements.** From
  `camt.053.001.07`, `Ntry/Sts` is a choice (`<Sts><Cd>BOOK</Cd></Sts>`) rather
  than a bare code. The parser read the raw inner text and produced
  `EntryStatus::Other("<CD>BOOK</CD>")`. Both shapes are now accepted, as are
  the v07 `RltdPties/Dbtr/Pty/Nm` party nesting and the v03 `BIC` → `BICFI`
  rename.
- **Batch-booked camt.053 entries lost all but one transaction.** Only the first
  `TxDtls` was read, discarding the rest of a batched SEPA collection. All
  transaction details are now exposed via `Camt053Entry::details`.
- `Stmt/Id` was resolved by an unanchored search that could match `Stmt/Acct/Id`
  instead. Element lookups are now anchored to the schema path.
- `Node::descendant` searched breadth-then-depth, returning the shallowest match
  rather than the first one in document order.
- Identifiers (`MsgId`, `PmtInfId`, `EndToEndId`, `MndtId`) were never checked
  against the SEPA character set — not even under `CharsetPolicy::Strict`, which
  documents a hard guarantee. `MndtId="MND-Straße"` built successfully and was
  then rejected by the bank.
- `total_ct()` summed without overflow checks and is callable before
  `validate()`, so a crafted batch could panic in debug or wrap to a negative
  `CtrlSum` in release. It now saturates; `validate()` still reports
  `ControlSumOverflow`.
- `is_valid_iso_date` accepted year `0000` (and `0000-02-29`, since year zero
  tests as a leap year). `xs:date` has no year zero, so `build()` succeeded and
  produced XSD-invalid XML.
- A `<Ntry>` carrying `<TxDtls>` directly, without the `<NtryDtls>` wrapper, lost
  all transaction detail. The old parser tolerated this shape; the tolerance is
  restored.
- `Node::code()` preferred stray text over a structured `Cd`/`Prtry` child, so
  `<Sts>STRAY<Cd>BOOK</Cd></Sts>` read as `STRAY`.
- **The `serde` feature alone did not compile.** The serde round-trip tests use
  `serde_json` but were gated on `serde` only, so they relied on the *optional*
  `json` dependency happening to be enabled. `serde_json` is now a
  dev-dependency, and CI gained a feature-combination matrix so this cannot
  recur. (Pre-existing; CI only ever ran `--all-features` and
  `--no-default-features`.)

- **Transliteration diverged from EPC217-08 in 154 places.** Ligatures expanded
  when the table maps them one-to-one (`Æ` gave `AE`, the EPC says `A`), and all
  129 Greek and Cyrillic letters collapsed to `.` — discarding the 26 published
  romanisations, so `Ψυχή` became `....` instead of `PSychi`. The
  `Transliteration::Epc` doc comment asserted the opposite of what the table
  says. Verified by diffing the implementation against the spreadsheet, and by
  an exhaustive sweep of all 1,114,112 code points × both styles confirming
  every output is SEPA-legal.
- `ct_from_eur_str` sliced the fractional part by byte index and could panic on
  a multi-byte character. The XML layer decodes `&#8364;` to `€` before this
  sees it, so `<Amt>1.&#8364;5</Amt>` in a bank file crashed the process.
- The XML parser accepted documents with **more than one root element**, and the
  last one won — a document-substitution vector, since appending a second
  `<Document>` made this library read different data than a validator saw.
  Multiple roots and unmatched end tags are now rejected.
- Identifiers (`MsgId`, `PmtInfId`, `EndToEndId`, `MndtId`) were never checked
  against the SEPA character set, not even under `CharsetPolicy::Strict`.
  They are now rejected rather than transliterated — an identifier is the key
  the bank echoes back, so silently rewriting it would break reconciliation.
- `write_xml_to` / `write_xml_to_io` bypassed validation entirely. Replaced by
  `write_to`, which validates before writing anything.
- `total_ct()` could overflow; `Node::descendant` returned the shallowest match
  rather than the first in document order; `is_valid_iso_date` accepted year
  `0000`; a `<Ntry>` carrying `<TxDtls>` without the `<NtryDtls>` wrapper lost
  all detail; `Node::code()` preferred stray text over a `Cd` child.

### Added

- **camt.054 XML parsing** (`parse_camt054`). The module was named `camt054` and
  documented as the Bank-to-Customer Notification parser, but could only parse a
  bespoke JSON shape — it could not read a camt.054 document at all.
  `Camt054Notification::returns()` surfaces returned direct debits, which is the
  main reason to consume camt.054.
- **camt.052 XML parsing** (`parse_camt052`) for intraday account reports, with
  `Camt052Report::pending_entries()` to separate provisional entries from booked
  ones.
- **A shared `camt` module.** camt.052, camt.053 and camt.054 now share one
  entry model (`CashEntry`, `EntryDetail`, `EntryStatus`, `StatementBalance`),
  so reconciliation code works across all three. `Camt053Entry` is renamed
  `CashEntry`.
- **The real EPC217-08 conversion table.** Transliteration was a hand-written
  approximation; it is now transcribed verbatim from the spreadsheet the EPC
  publishes — 1011 mappings across Latin, Latin Extended, Greek, Cyrillic and
  symbols, in `charset_table.rs`. No other library in any language surveyed
  implements this table.
- **Multiple `PmtInf` blocks per message** — the builders now carry
  [`CreditTransferGroup`] / [`DirectDebitGroup`] blocks instead of a single flat
  batch. Sequence type, execution date, debtor/creditor account, batch booking
  and category purpose all live at that level, so one file can now mix `FRST`
  with `RCUR`, or carry several execution dates. Previously each combination
  needed its own file and its own submission to the bank — the documented
  workaround was "use multiple builders", which does not produce one message.
- **Reproducible output** — `created_at` pins `CreDtTm`, so a message can be
  regenerated byte-identically for a golden-file test or an audit. It defaulted
  to `SystemTime::now()` with no way to override.
- **`BtchBookg` control** — `batch_booking(bool)` per group. Omitted by default,
  which defers to the agreement with the bank; there is no scheme-wide default,
  and German banks read an absent value as `true`.
- **Ultimate parties** ([`Party`]) — `UltmtDbtr` / `UltmtCdtr`, for payments on
  behalf of a third party. Restricted to the two children SEPA permits (`Nm`,
  `Id`); `PstlAdr`, `CtryOfRes` and `CtctDtls` are stripped from the party type
  by the DK schema and are not emitted. `Nm` is capped at the EPC's 70
  characters, not the schema's 140.
- **Purpose codes** — [`Purpose`] (`Purp/Cd`) and [`CategoryPurpose`]
  (`CtgyPurp/Cd`) as **separate** enums, because they are different code sets.
  `RENT` is a purpose and not a category purpose; `DIVI` (category) and `DIVD`
  (purpose) both mean "dividend" and are not interchangeable. Unknown-but-
  well-formed codes are carried through rather than rejected, since ISO revises
  these lists quarterly.
- **SDD mandate amendment** ([`MandateAmendment`]) — `AmdmntInd` /
  `AmdmntInfDtls` for a changed debtor account, creditor identifier, creditor
  name or mandate reference. Emits the `SMNDA` marker in its current position
  (`OrgnlDbtrAcct`), which moved there in EPC IG v9.0; the pre-2016 position
  under `OrgnlDbtrAgt` is not emitted.
- **Fuzz targets** (`fuzz/`) — three `cargo-fuzz` harnesses over the parsers,
  the identifier validators and the builders. Two of the panics fixed in this
  release were found this way. The `identifiers` target asserts that
  transliteration output is always SEPA-legal, so it catches silent corruption
  and not just crashes.
- **ISO 11649 RF Creditor Reference** (`RfReference`) — validation *and*
  generation. No library in any language surveyed generates RF check digits;
  `python-stdnum` validates but cannot generate.
- **Structured remittance information** (`RemittanceInfo`), emitting
  `RmtInf/Strd/CdtrRefInf` with `Cd=SCOR` and `Issr=ISO`, XSD-validated.
  `CreditTransferEntry::with_reference` / `DirectDebitEntry::with_reference`.
  The block is emitted minified: the EPC caps the whole `Strd` element at 140
  characters including tags, which pretty-printing alone overruns.
- `Camt053Statement::account_servicer_bic`, and the same on the new report and
  notification types.
- Strict lint floor: `clippy::pedantic`, `clippy::cargo`, plus `unwrap_used`,
  `expect_used`, `panic`, `indexing_slicing` and `missing_panics_doc` denied in
  library code (relaxed inside tests, where `unwrap` is the assertion).

- `validate` module: EPC field rules the XSD does not enforce — amount range
  (0.01 – 999,999,999.99 EUR), identifier and name lengths, EPC230-15 slash
  rules, real calendar dates, non-empty batches.
- `charset` module: SEPA Basic Latin set plus transliteration in two styles —
  `Transliteration::German` (`ä→ae`, DK-sanctioned, the default) and
  `Transliteration::Epc` (`ä→a`, the strict EPC217-08 one-to-one table).
- `CharsetPolicy::Strict` to reject non-SEPA characters instead of rewriting them.
- `iban::is_sepa_country` and `Iban::is_sepa`, over the 42 codes in EPC409-09
  v8.0 — including the 2025 additions (AL, MD, ME, MK, RS) and correctly
  excluding the Danish territories FO and GL.
- `creditor_id::creditor_id_check_digits` to generate check digits.
- `Bic::is_passive` and `Bic::is_primary_office`.
- `Camt053Entry`: `currency`, `batch_booked`, `details`, and accessors
  (`end_to_end_id()`, `reference()`, …) delegating to the first detail.
- `StatementBalance::currency` — camt.053 is not EUR-only.
- `DirectDebitSchema`, adding `pain.008.001.08` support.
- XSD validation in the test suite: generated documents are checked against the
  real ISO 20022 schemas with `xmllint`, and CI installs it so the checks run.
- README examples are compile-tested as doctests.

### Changed — breaking

- **`Pain001Builder::new` and `Pain008Builder::new` take only the initiating
  party.** The debtor/creditor account moves to the group, and for pain.008 the
  Creditor Identifier is a required argument of
  [`DirectDebitGroup::new`] — the EPC mandates `CdtrSchmeId`, so there is no
  valid group without one, and the type system now says so.
- Per-group setters (`execution_date`, `collection_date`, `sequence_type`,
  `scheme`, `debtor_bic`, `creditor_bic`, `local_instrument`) moved from the
  builder to the group.
- `PmtInfId` defaults to the `MsgId` for a single group and to `MsgId-<n>` for
  several, truncated to stay inside `Max35Text`, and is validated either way.
- Setting the same ultimate party at both group and transaction level is now an
  error (`ValidationError::ConflictingLevels`) — the DK rules require one level
  or the other.
- `Camt053Entry` → `CashEntry`, re-exported from the shared `camt` module.
- `CreditTransferEntry::description` / `DirectDebitEntry::description` →
  `remittance: Option<RemittanceInfo>`. `with_description` still works and now
  wraps the text as `RemittanceInfo::Unstructured`.
- `write_xml_to` / `write_xml_to_io` → `write_to`, returning `WriteError`.

- **`build_xml() -> String` is replaced by `build() -> Result<String, ValidationError>`**
  on both builders. Invalid batches are now rejected rather than silently
  emitted. `validate()` runs the same checks without serialising.
- **Default schema versions moved to the current SEPA releases**:
  `pain.001.001.09` (was `pain.001.003.03`) and `pain.008.001.08` (was
  `pain.008.003.02`, previously the only option). The DK variants had been
  end-of-life since November 2022; they remain available via `.schema(…)`.
- **A Creditor Identifier is now required for pain.008.** The EPC rulebooks
  mandate `CdtrSchmeId`; batches without one fail with
  `ValidationError::MissingCreditorId`.
- **Text is transliterated into the SEPA character set by default.**
  `Müller & Söhne GmbH` is emitted as `Mueller + Soehne GmbH`. Opt out with
  `.charset(CharsetPolicy::Strict)`.
- `Camt053Entry`'s transaction-level fields (`end_to_end_id`, `mandate_id`,
  `creditor_id`, `reference`, `counterparty_name`, `counterparty_iban`,
  `return_reason_code`) moved into `EntryDetail` and are now **methods** rather
  than fields.
- `CreditorIdError::InvalidChecksum` carries `{ expected, actual }` instead of a
  raw remainder.
- `BicError::InvalidInstitutionCode` and `BicError::InvalidCountryCode` are
  removed; malformed BICs now report `InvalidCharacter { ch, pos }`. BIC
  validation follows the SEPA pattern
  `[A-Z]{6}[A-Z2-9][A-NP-Z0-9]([A-Z0-9]{3})?`, which is stricter than
  ISO 9362:2022 — a BIC the standard allows but SEPA schemas reject is refused
  here, since accepting it would only produce unusable files.
- **`write_xml_to` / `write_xml_to_io` are replaced by
  `write_to(&mut impl io::Write) -> Result<(), WriteError>`.** The old writers
  bypassed validation entirely, happily emitting an empty `MsgId`, a
  `NbOfTxs` of 0 or a malformed date that `build()` rejected on the same
  builder. `write_to` validates *before* writing, so a rejected batch leaves the
  writer untouched instead of producing a truncated file. The `fmt::Write`
  serialiser is now private.
- Identifiers are **never transliterated**, under any `CharsetPolicy`. An
  identifier is the key the bank echoes back in pain.002 and camt.05x, so
  silently rewriting it would break the caller's own reconciliation; out-of-set
  characters are a hard error instead. Names and remittance text are still
  transliterated by default.
- The EPC230-15 slash rules are now checked on the trimmed value, so leading or
  trailing whitespace can no longer smuggle a `/` past them.
- `Pain002ParseError` and `Camt053ParseError` gain an `Xml(XmlError)` variant.
- `PmtInfId` no longer carries a `-1` suffix; it defaults to the `MsgId`. Use
  `payment_info_id(…)` to set it explicitly.
- The `SCT Inst` 100,000 EUR cap is **not** enforced: it was removed from the
  scheme on 5 October 2025 under Art 5a(6) of the amended SEPA Regulation.

### Security

- `<!DOCTYPE>` declarations are rejected, foreclosing entity-expansion
  ("billion laughs") attacks. `quick-xml` never resolves external entities, so
  XXE was not reachable; this is defence in depth.
- XML nesting is capped at 256 levels, and the tree is built with an explicit
  stack so deep input cannot overflow the call stack.
- Unknown entity references are an error rather than being passed through
  undecoded into a payment field.
- Documents with multiple root elements are rejected, closing a
  document-substitution vector (see above).
- Amount parsing can no longer panic on multi-byte input from a bank file.

## [0.3.0]

- Added SEPA Creditor Identifier validation.
- Added pain.008 scheme variants (CORE / B2B).

## [0.2.0]

- Added SEPA Creditor Identifier validation.

## [0.1.0]

- Initial release: IBAN/BIC validation, pain.001 and pain.008 builders,
  pain.002, camt.053 and camt.054 support.
