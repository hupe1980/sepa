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

Pin `sepa = "0.8"` and treat a move to `0.9` as a deliberate migration.

## [0.8.0]

**Three themes, and they rhyme: every one is something the suite could not
see.** The read path stopped guessing; a fifth EPC payment scheme turned out to
be missing; and the artefacts outside this repository became checkable.

> **Migration, in three parts.**
>
> 1. The money fields on every camt type move into one `amount:
>    ReportedAmount`, and `signed_ct()` returns `Option<i64>`:
>    `entry.amount_ct` → `entry.amount.ct`, `entry.currency` →
>    `entry.amount.currency`, `entry.indicator` → `entry.amount.direction`,
>    each with a `*_raw` field beside it. A `None` means *the statement did not
>    determine this* — escalate the row, never substitute a figure.
> 2. `LocalInstrument` is now `CreditTransferKind` and
>    `CreditTransferGroup::local_instrument` is `::kind`.
>    `LocalInstrument::None` → `CreditTransferKind::Standard`,
>    `LocalInstrument::Inst` → `CreditTransferKind::Instant`. The compiler
>    finds every site; emitted output for both is unchanged.
> 3. Nothing else. The value layer is untouched.

### Emitted output

- **New: OCT Inst documents.** A group with
  `CreditTransferKind::OneLegOutInstant` emits `SvcLvl/Cd=EOLO`, a mandatory
  `LclInstrm/Cd=INST`, a non-`SLEV` `ChrgBr`, optionally a non-euro
  `InstdAmt/@Ccy`, and optionally `InstrForCdtrAgt/InstrInf`.
- **Otherwise unchanged.** Every document 0.7 produced, 0.8 produces
  byte-for-byte. `ChrgBr` still renders `SLEV` for all four SEPA schemes — it
  is a value now rather than a literal, and `build()` refuses any other under a
  SEPA service level.

- **New: OCT Inst documents.** A group with
  `CreditTransferKind::OneLegOutInstant` emits `SvcLvl/Cd=EOLO`, a mandatory
  `LclInstrm/Cd=INST`, a non-`SLEV` `ChrgBr`, optionally a non-euro
  `InstdAmt/@Ccy`, and optionally `InstrForCdtrAgt/InstrInf`. Nothing else
  changes shape.
- **Unchanged for SCT, SCT Inst, SDD Core, SDD B2B and every reversal, recall
  or existing document.** `ChrgBr` still renders `SLEV` for all of them —
  it is now a value rather than a literal, and `build()` refuses any other
  under a SEPA service level.

### Added — One-Leg Out Instant Credit Transfer (OCT Inst)

The fifth EPC payment scheme, in force since 5 October 2025 (rulebook
EPC158-22, customer-to-PSP guidelines EPC250-22 2025 v1.0). It covers the euro
leg of an instant payment whose other leg leaves SEPA, and it adds no message:
EPC250-22 specifies `pain.001.001.09`, `pain.002.001.10` and
`camt.054.001.08`, all already implemented. What it adds is rules.

- **`CreditTransferKind`** replaces `LocalInstrument` and names the scheme
  rather than one of its elements: `Standard`, `Instant`, `OneLegOutInstant`.
  One enum rather than two fields, because the combinations are not
  orthogonal — `EOLO` without `INST` is not a scheme, and neither is `EOLO`
  with `SLEV`.
- **`ChargeBearer`** and `CreditTransferGroup::charge_bearer`. SEPA mandates
  `SLEV`; OCT Inst forbids it and allows `CRED`, `DEBT`, `SHAR` (default
  `SHAR`). Either violation is `ValidationError::ChargeBearerNotAllowed`.
- **`Currency`** — a validated ISO 4217 code — and
  `CreditTransferEntry::with_currency`, to order an amount in one. Reserved
  codes that match `[A-Z]{3}` and name no money (`XXX`, `XTS`, the four
  metals) are refused; there is no table of active codes.
- **`CreditTransferEntry::with_non_euro_leg_currency`** — AT-T020, which
  EPC250-22 carries in `InstrForCdtrAgt/InstrInf`.
- Cross-scheme combinations are refused by name: a non-euro amount or an
  AT-T020 instruction under SEPA, and `EOLO` on a pre-2019 schema.
- A timed execution (`ReqdExctnDt/DtTm`) now follows the scheme rather than
  the local instrument, so OCT Inst can carry one.

### Added — the regulatory watch list, as a gate

Every other check here measures the crate against an artefact *inside* the
repository, which cannot notice a publisher issuing a newer one. Two defects
reached a release through that gap with the whole suite green.

- **`tests/watch.rs`** pins each external publication — IBAN Registry release,
  rulebook version, guideline, code list — with the artefact in force, when it
  was last read, how long that stays good, and the files it reaches. It fails
  when one falls due, naming the source and what to look for. Consumer version
  pins are checked the same way, against the sibling manifests.
- Run by `just watch` and a CI job of its own. `#[ignore]`d in the default
  suite: an overdue reading is a human task, not a reason to fail an unrelated
  pull request. Excluded from the published package.
- A row pointing at a file that no longer exists fails, so it cannot outlive
  what it describes.

### Added — a schema gate for inline fixtures

- **`tests/fixtures.rs`** walks `src/` for ISO 20022 document literals and
  validates each against the schema its own namespace names, so coverage does
  not depend on a fixture being added to a list. Deliberately invalid fixtures
  opt out with `// xsd-exempt: <reason>`.
- It found **eighteen fixtures no bank could have sent**: missing mandatory
  `CreDtTm`, `Bal`, `Sts`, `BkTxCd`, `Amt`, `CdtDbtInd`, `OrgnlMsgNmId`, and
  five element-order errors — plus the parser defect below.
- Three more schemas vendored so nothing is skipped: `camt.053.001.06`,
  `pain.002.001.03`, `pain.002.003.03`. Each is checked to **reject** as well
  as accept; `pain.002.003.03` is single-sourced, so that pair is its only
  bound.
- **The legacy DK status report is reject-only.** `pain.002.003.03` restricts
  every status to `RJCT` and drops proprietary reasons and `AddtlInf`. Three
  fixtures asserting acceptances under it described documents that cannot
  exist; they move to `pain.002.001.03`, and a genuine DK reject report now
  covers the variant.

### Fixed — a proprietary code the parser could not read

- **`<Prtry><Id>…</Id></Prtry>` read as `None`.** `Node::code` handled
  `Prtry` only as text, which is its shape in `BalanceType10Choice`,
  `EntryStatus1Choice` and `ReturnReason5Choice`. In `ChargeType3Choice` it is
  a `GenericIdentification3` whose `Id` is **mandatory**, so every
  schema-valid proprietary charge type — a return fee, typically — was
  discarded. `BkTxCd/Prtry`, which nests a `Cd`, had the same problem. Both
  shapes now resolve.

  The fixture that should have caught it wrote `<Prtry>RETURN_FEE</Prtry>`,
  which no schema admits; the new inline-fixture gate is what surfaced it.

### Fixed — the control sum

- **The control sum was bounded by `i64`, not by the schema.** `CtrlSum` is a
  `DecimalNumber` with `totalDigits="18"` — 9,999,999,999,999,999.99 EUR with
  two fraction digits. The guard was a bare `checked_add`, bounded by
  `i64::MAX`, which is a nineteen-digit value. Between the two lay a window in
  which individually legal entries summed to a file that passed this crate's
  validation and failed `xmllint`. All three writers now share
  `validate::accumulate_control_sum`.

### Changed — error messages

- `ValidationError::ControlSumOverflow`'s message names the schema bound rather
  than `i64`.
- The timed-execution error's `requires` text names the scheme rather than
  `PmtTpInf/LclInstrm = INST`, because both instant schemes now allow one.

### Fixed — money

- **An unreadable `ChrgInclInd` is no longer indistinguishable from an absent
  one.** `included_in_amount` is `Option<bool>` and `None` was documented as
  "the bank did not say", but it also swallowed values outside the four
  `xs:boolean` forms — `TRUE`, `yes`. That field decides whether a ledger
  posts a charge or treats it as already inside the entry amount, so losing
  the distinction is a double-count waiting for a bank that spells it
  differently. `ChargeRecord::included_in_amount_raw` keeps what arrived.
- **An unreadable `CdtDbtInd` is no longer read as a credit.** `indicator_of`
  ended in `.unwrap_or(CreditDebitIndicator::Credit)`, so an entry whose
  credit/debit indicator was absent, empty or misspelled — `DBTI` for `DBIT` —
  was reported as a **credit of the same magnitude**. In a ledger that is a
  two-for-one error: a EUR 1,000 debit became a EUR 1,000 credit, a EUR 2,000
  swing, from one mistyped character in a third party's file. It applied to
  entries, to balances (an overdraft read as funds) and to transaction details.
  `CreditDebitIndicator` has also lost its `Default` derive — a money direction
  has no default.
- **An entry is no longer dropped when its amount cannot be read.**
  `parse_entry` and `parse_balance` returned `Option` and were collected with
  `filter_map`, so a booking whose `Amt` this crate could not represent
  **vanished from the statement** with no error. That is the one parse failure
  an importer cannot detect: a missing booking is indistinguishable from one
  that never happened. Entries, balances and charge records are now always
  reported, carrying `None` amounts and the verbatim text.
- **Sub-cent amounts are refused instead of truncated.** `ct_from_eur_str`
  truncated below two decimals, so `0.001` parsed as `0` and `1.999` as `199`.
  `ActiveOrHistoricCurrencyAndAmount` permits five fraction digits, so both are
  reachable from a bank file. New `AmountError::SubCentPrecision`. Trailing
  zeros remain insignificant: `"1.500"` is still `150`.
- **A leading `+` is accepted.** `+1000.00` is legal `xs:decimal` and banks send
  it; it was rejected, and — via the bug above — took its whole entry out of the
  statement with it. A repeated sign is still rejected.
- **`net_movement_ct()` is all-or-nothing.** It summed with `saturating_add`
  over entries that silently resolved to zero or the wrong sign. A total that
  skips the rows it could not read looks right and is not.
- **`details_reconcile()` no longer reports agreement with an unreadable
  entry.** It compared against a fabricated entry total; with no total to
  compare against, it now answers `false`.
- **An absent `Amt/@Ccy` is no longer read as `EUR`.** camt statements are not
  EUR-only, and the fabricated currency propagated: a detail is excluded from
  its entry's sum when the two differ, so guessing here silently changed which
  transactions were counted.
- **A `Bal` with no `Tp` is `BalanceType::Unspecified`**, not `Other("")` —
  which is what a bank sending an empty code produces. Two different facts had
  been collapsed into one value.
- **A `NbOfTxsPerSts` row survives a count it cannot read.** The whole row was
  dropped, losing the bank's assertion that the status bucket exists;
  `StatusCount::count` is now `Option<u64>` with `count_raw` beside it.

### Fixed — lexical conformance

Two places where the crate was **stricter than the standard it names**, which is
the direction where nothing fails visibly: the caller is refused, at the call
site, with nothing they can do.

- **`IsoDate` rejected a legal `xs:date` timezone.** `xs:date` is
  `'-'? yyyy '-' mm '-' dd zzzzzz?` — the zone is optional but legal, so
  `2026-07-20Z` and `2026-07-20+02:00` are schema-valid `ISODate` values that
  this crate could not read. Both are accepted now; the zone is dropped, since
  every date the builders write is a bare `xs:date`.
- **`IsoDate::parse_date_part` accepted anything after the tenth character.**
  `2026-07-20GARBAGE` returned a confident booking date. It now accepts exactly
  the two members of `DateAndDateTimeChoice` — `xs:date` and `xs:dateTime` —
  and nothing else.
- Found by the seeded fuzzer, in code added by the fix above: the timezone check
  computed `byte - b'0'` before verifying the byte was a digit, so
  `2026-07-20+!!:!!` **panicked**. Reachable from any parsed date.

### Fixed — other

- `IsoDate::today()` and `IsoDateTime::now()` saturated to **`0001-01-01`** on a
  clock past year 9999 — wrong, and wrong in the opposite direction from the
  fault. They saturate forward now.
- `PostalAddress` looked up its length bounds with a silent
  `.unwrap_or(MAX_NAME_LEN)`, so an address element added to the writer without
  a bound in `validate::max_text_len` would have inherited a plausible 70. The
  element list and the table are now checked against each other by a test —
  D37 applied where it had been skipped.

### API

Breaking, with no deprecation path, and the compiler finds every site:

| Was | Is |
|---|---|
| `CashEntry::signed_ct() -> i64` | `-> Option<i64>` |
| `StatementBalance::signed_ct() -> i64` | `-> Option<i64>` |
| `ChargeRecord::signed_ct() -> i64` | `-> Option<i64>` |
| `net_movement_ct() -> i64` (camt.052/053/054) | `-> Option<i64>` |
| `amount_ct: i64` | `amount_ct: Option<i64>`, plus `amount_raw: Option<String>` |
| `currency: String` | `currency: Option<String>` |
| `indicator: CreditDebitIndicator` | `indicator: Option<CreditDebitIndicator>`, plus `indicator_raw: Option<String>` |
| `CreditDebitIndicator: Default` | removed |
| `CashEntry`/`StatementBalance`/`ChargeRecord`/`EntryDetail` money fields | one `amount: ReportedAmount` |
| `StatusCount::count: u64` | `count: Option<u64>`, plus `count_raw: Option<String>` |
| — | `BalanceType::Unspecified` |
| `Pain002Document::created_at: String` | `created_at_raw: String`, plus `created_at() -> Option<IsoDateTime>` |
| `Camt029Document::created_at: String` | `created_at_raw: String`, plus `created_at() -> Option<IsoDateTime>` |
| — | `AmountError::SubCentPrecision { value, digits }` (`#[non_exhaustive]`, so no `match` breaks) |

The shape is not new: `EntryDetail::signed_ct()` has returned `Option<i64>`
since 0.5, for exactly this reason. What 0.8 does is apply it one level up,
where the same question was being answered with a guess. The `created_at`
rename is the same correction applied to the other direction — camt.05x gained
"verbatim *and* typed" in 0.7 and pain.002/camt.029 did not.

### Testing — why none of this was caught

Three separate mechanisms, and the new gates target each one.

- **Two of the defects were asserted as correct.** The truncation and the
  dropped entry both had tests pinning them down — written to lock in a *panic*
  fix in 0.6, recording whatever the non-crashing behaviour happened to be
  without asking whether it was right. A regression test for a crash encodes the
  recovery as the specification.
- **The sign flip was never exercised.** All 25 `CdtDbtInd` occurrences in the
  entire fixture corpus were a correctly spelled `CRDT` or `DBIT`. The invalid
  branch of a two-branch enum had no coverage at all, because every fixture was
  hand-written by somebody who knew the right codes.
- **The fuzzer could not see any of them.** The `parse` target's whole invariant
  was "does not panic" — every accessor result was discarded with `let _ =`. A
  fabricated value is a perfectly ordinary `Ok`.

The gates added in response:

- **`tests/conformance.rs`** — 22 tests over the three classes the rest of the
  suite is blind to: input the crate wrongly **refuses** (oracle: the XML
  Schema lexical spaces for `xs:decimal`, `xs:date`, `xs:dateTime` and
  `xs:boolean`), values it **invents** (oracle: the input bytes), and elements
  it **drops** (oracle: the input element count). Each gate was verified by
  reintroducing the defect it targets and confirming it fails.
- **The `parse` fuzz target asserts the money invariants**, not just absence of
  panics: a ledger figure exists exactly when its magnitude and its direction
  both do, signing changes only the sign, and a resolved direction must
  round-trip to the text it was read from.
- **`fuzz/seeds/`, checked in** — and load-bearing rather than an optimisation.
  Random bytes are never a well-formed ISO 20022 document, so an unseeded
  `parse` run explores only the XML rejection path: **315,449 unseeded
  executions did not rediscover the sign-flip defect, and a seeded run finds it
  in seconds.** `just fuzz-seeds` regenerates them from the shared fixtures, and
  CI fails if the committed seeds no longer match — a generated artefact that is
  committed and never re-derived is a comment.

### Internal

- The four camt types that carry money now embed a single `ReportedAmount`
  instead of repeating five fields and an accessor with identical semantics.
  The invariant has one definition, so a new money-bearing type inherits it.

### Documentation

- The **EPC withdrew the 15 November 2026 unstructured-address end-date** on
  9 September 2026, after Swift extended its own migration period on 27 August.
  A new date is to be set in October 2026. That date was asserted as settled
  fact in 24 places — the README, the crate root, seven modules, the examples
  and five site pages. Nothing in the code changed, because `PostalAddress` was
  built around which forms are durable rather than around when the others
  expire; the claims are now pinned to the rule, and the date itself is stated
  once, in `address`, with its history.
- **A migration to a newer ISO 20022 version is now on the EPC's roadmap** —
  change request 6 of the 2026 cycle, recommended for November 2029. The crate's
  long-standing "nothing on the EPC roadmap moves SEPA past them" is retired;
  the decision to emit the mandated versions is unchanged.
- Verification of Payee rulebook v1.1 (effective 20 September 2026) was read in
  full: `AT-R001` still carries exactly four outcomes, and nothing this crate
  parses changes.
- Internal decision identifiers were removed from public rustdoc and the site.
  They referenced a directory that is not published.

## [0.7.0]

The largest revision since 0.4, and two bodies of work in one release.

**Correctness and determinism.** One text field escaped the SEPA character set
entirely, one comparison was chronologically wrong, one acceptance check
accepted silence, one validator was **rejecting real BICs**, and the builders
defaulted the two values that must never be defaulted.

**The message set closed.** `camt.055` recalls and `camt.029` resolutions are
the last message-level gap, and vendoring the camt schemas to gate them found a
parser fixture that was not a valid document.

**Breaking API changes with no deprecation path.**

> **Migration in one line.** `MsgId` and the payment date move into the
> constructors: `Pain001Builder::new(party, msg_id)`,
> `Pain008Builder::new(party, msg_id)`,
> `Pain007Builder::new(party, original_msg_id, msg_id)`,
> `CreditTransferGroup::new(name, iban, execution)` and
> `DirectDebitGroup::new(name, iban, creditor_id, collection_date)`. The
> `.msg_id()`, `.execution_date()`, `.execution_at()` and `.collection_date()`
> setters are gone — pass the value where the type now demands it. A camt
> reader has three more: `from_date`, `to_date` and `created_at` are now
> `*_raw` fields with typed accessors of the old names beside them.

### Emitted output

- **New on the wire:** `camt.055.001.05` payment cancellation requests.
- `SchmeNm/Prtry` and a proprietary remittance `Issr` are now transliterated
  under the default `CharsetPolicy`, where they were emitted verbatim. For input
  that was already inside the SEPA character set, nothing changes.
- Length limits are measured on the trimmed value — see
  [Changed — emitted output](#changed--emitted-output) below.
- Everything else is byte-identical to 0.6 for any input 0.6 accepted.

### API

Breaking, with no deprecation path — see the migration note above. Three error
variants are added (`SchemaPattern`, `DateOrder`, `ConversionError`) and, since
every error type is `#[non_exhaustive]`, no `match` breaks on them.

### Added — camt.055 recall and camt.029 resolution

The last message-level gap. `Camt055Builder` asks the bank to stop a submission
before it settles; `parse_camt029` reads the answer. Both are XSD-validated in
CI against the ISO originals the DFÜ-Abkommen names.

Three cancellation scopes — the whole file, a whole `PmtInf`, or named
transactions — and they are **alternatives**. The schema makes every part of
`UnderlyingTransaction12` optional, so "cancel the whole message, and also
specifically these two" validates cleanly and is not an instruction a bank can
action; `build()` refuses the combination, and refuses a scope that names
nothing at all.

Three more rules the XSD does not carry:

- A **reason is a constructor argument**. ISO types `CxlRsnInf` as optional; no
  bank can act on a reasonless recall, so the reasonless form is not something
  this crate can be asked to emit.
- `CancellationReason5Code` is a **closed** enumeration, unlike the purpose code
  lists. An unrecognised code in `Cd` is schema-*invalid*, not merely unknown,
  so `CancellationReason::Other` writes to the `Prtry` branch instead.
  `is_iso_code()` says which branch a value takes.
- `CtrlData/NbOfTxs` counts what the *message* lists, so a whole-file or
  whole-group recall writes no `CtrlData` rather than asserting a zero.

`OriginalMessage::from_direct_debit` / `from_credit_transfer` take the
**builder**, not the XML — every element the recall needs is already in it, and
re-parsing a document to recall it would be reading back what the caller has.
When the original pinned no `created_at`, `OrgnlCreDtTm` is omitted rather than
stamped with "now": the element names the moment the original was created.

On the reading side, `Camt029Document` repeats the pain.002 level structure and
its failure mode. A refusal at message level carries **no transaction blocks at
all**, so `is_accepted()` treats an empty document as *not* an acceptance and
`rejection_reasons()` gathers all three levels. `PDCR` — pending — is neither
outcome, and `is_final()` exists for that one code: posting it as accepted
writes off money that is coming back, posting it as rejected collects twice.
`RejectionReason::is_too_late` separates `ARDT` ("we could not — it settled,
use pain.007") from `LEGL`/`CUST` ("we would not").

Two builder accessors came with it, and are useful on their own:
`Pain001Builder`/`Pain008Builder` now expose `message_id()`, `schema_version()`
and `creation_timestamp()`.

### Fixed — `validate_bic` rejected real BICs

**ISO 9362:2022 §6.3.1 widened the business party prefix from four letters to
four alphanumerics**, and SWIFT allocates BICs under it. `validate_bic`
enforced `[A-Z]{6}` over the first six characters — the *pre-2019 ISO 20022*
pattern, not the standard — so `E097AEXX` was refused. That is a loud failure at
the caller with nothing the caller can do about it.

Relaxing it alone would have been wrong in the other direction: `pain.008.001.02`
and the three other pre-2019 schemas genuinely cannot hold such a BIC. So the
two facts are now separate:

- `validate_bic` enforces the current standard —
  `[A-Z0-9]{4}[A-Z]{2}[A-Z0-9]{2}([A-Z0-9]{3})?` plus a real country code.
- `BicPattern` (`Alphanumeric` / `LettersOnly`) is the pattern a *schema* uses,
  `Bic::fits(pattern)` asks whether a value satisfies it, and `build()` refuses
  a mismatch with the new `ValidationError::SchemaPattern` — naming the XSD
  pattern string, because two different ISO 20022 types are both called
  `BICFIIdentifier` with different patterns.

The element name is not the signal, which is what camt.055 proved:
`pain.008.001.08` writes `BICFI` over the wide pattern, `pain.008.001.02` writes
`BIC` over the narrow one, and `camt.055.001.05` writes `BICFI` over the
**narrow** one. Each schema states both facts independently.

### Fixed — a fixture that no bank would ever send

camt was the only message family with no schema gate: every claim about the
parsers rested on documents this repository had written. Five camt schemas are
now vendored under the same digest enforcement as the ten pain ones, and the
shared read-path fixtures go through `xmllint`.

It paid for itself immediately. The camt.053 batch-booking fixture put `Amt`
before `Refs` inside `TxDtls`, and `EntryTransaction10` sequences `Refs` first —
so it had been standing in for a real statement for three releases while being
a document no bank sends. Nothing was mis-parsed, because the parser is keyed by
local name and does not care about order; but the evidence was not evidence.

### Fixed — `ct_to_eur_str` and `ct_from_eur_str` disagreed about their range

`ct_to_eur_str(i64::MIN)` printed `-92233720368547758.08` and
`ct_from_eur_str` rejected that string as an overflow: the magnitude was built
positive and negated afterwards, and `i64::MIN` has no positive counterpart. The
parser now accumulates in the sign the input asked for and is exact over the
whole `i64` range.

That fix made `i64::MIN` *reachable* from a bank file, where every `.abs()` and
unary `-` downstream would panic on it — so camt amounts are now normalised to a
magnitude once, in `amount_of`, which returns `None` for a value with no
magnitude in `i64`. `Camt053Statement::net_movement_ct` also stopped using
`Iterator::sum`, which panics on overflow in a debug build; it saturates like
its camt.052 and camt.054 counterparts always did.

### Fixed — `ẞ` transliterated to lower case

U+1E9E LATIN CAPITAL LETTER SHARP S shared an arm with `ß` under the German
style and produced `ss`, so `STRAẞE` came out `STRAssE` on a statement. It is
now `SS`. Under the EPC style it was falling through to the `.` fallback and
losing the letter entirely — the character was encoded in Unicode 5.1, after
EPC217-08 was drawn up, so the published table has no row for it; it now takes
the upper case of what its lower-case counterpart maps to.

### Fixed — a mandate could be signed after the collection it authorises

`DtOfSgntr` later than `ReqdColltnDt` describes a collection taken on the
authority of a mandate that did not yet exist. The debtor's bank answers `MD01`
and charges a return fee. This needs no clock and no banking calendar — both
values are in the document — and it is now
`ValidationError::DateOrder`, naming both elements and both values.

### Fixed — pain.007 accepted contradictory group identifiers

Two `OrgnlPmtInfAndRvsl` blocks naming the same `OrgnlPmtInfId`, or sharing a
`RvslPmtInfId`, left the reversal unattributable. Both are now
`ValidationError::Duplicate`, which is the rule `PmtInfId` already had in
pain.001 and pain.008.

### Added — IBAN generation

`iban_check_digits(country, bban)` and `Iban::from_bban(country, bban)` join
`creditor_id_check_digits` and `RfReference::check_digits_for`; building an IBAN
from a national bank code was the one check-digit scheme a caller still had to
paste a snippet for. `from_bban` runs the result back through `validate_iban`,
so the registry structure applies to a generated IBAN exactly as it does to a
parsed one, and the two share one mod-97 fold so they cannot disagree about the
arithmetic. Each of SWIFT's 78 published examples is now regenerated from its
own BBAN as well as validated.

### Added — the date interop the docs already promised

`IsoDateTime` converted *from* `time::PrimitiveDateTime` and
`chrono::NaiveDateTime` and back to neither, while the module documentation said
both directions. All four now exist, plus `time::OffsetDateTime` and
`chrono::DateTime<FixedOffset>` in both directions.

The offset-carrying conversions are the ones with an opinion:
`OffsetDateTime` and `DateTime` name an *instant*, and an `IsoDateTime` with no
offset does not — so that direction returns the new `ConversionError::NoOffset`
rather than assuming UTC. Same refusal `unix_seconds()` already made.

### Changed — camt documents keep dates verbatim *and* typed

`CashEntry::booking_date()` / `booking_date_raw` was the rule everywhere except
the document and statement headers, which kept only the raw string. `from_date`,
`to_date` and `created_at` on `Camt052Report`, `Camt053Statement`,
`Camt054Notification` and the three documents are now `*_raw` fields with typed
accessors of the old names beside them.

### Fixed — a remittance issuer bypassed the character set and every length rule

`RmtInf/Strd/CdtrRefInf/Tp/Issr` on the `RemittanceInfo::Proprietary` variant
was neither validated nor put through the `CharsetPolicy`. Any text at all
reached the wire: non-SEPA characters, and with no `Max35Text` bound — a
115-character issuer full of umlauts built and emitted without complaint. It is
now length-checked, charset-checked, and transliterated on write like every
other string in the crate.

The test that should have caught it could not: it asserted over a hand-written
list of tags, and nobody added `Issr` to the list. It now walks **every text
node** of a generated document, so the next element added to a writer is covered
the day it is added.

### Fixed — `Strd` overran the EPC's 140-character cap

The EPC limits structured remittance information to 140 characters *including
the XML tags*, which is why the block is emitted minified — and which nothing
checked. A 35-character `Ref` beside a 35-character `Issr` is two legal fields
and one illegal block. The limit is now enforced as
`ValidationError::TooLong { field: "RmtInf/Strd", max: 140, .. }`, measured by
rendering the block with the same function the writer uses, so what is checked
is what is emitted. New `validate::MAX_STRUCTURED_REMITTANCE_LEN`.

### Fixed — `IsoDateTime` compared as written fields, not as instants

`Ord` was derived over `(date, hour, minute, second, offset)`, so
`2026-07-20T13:00:00+02:00` — an hour *earlier* than `2026-07-20T12:00:00Z` —
sorted after it. A sort of bank-supplied timestamps was silently wrong whenever
offsets differed.

`PartialOrd` and `Ord` are **removed**. An offset-less timestamp names no
instant, so no total order over the type is correct, and one that is right for
same-offset values and wrong for mixed ones is worse than none. Two new methods
replace them:

- `IsoDateTime::unix_seconds() -> Option<i64>` — `None` for exactly the values
  that cannot be compared;
- `IsoDateTime::to_utc() -> Option<Self>` — the same instant in the `Z` form.

Equality stays on the written form, because a `CreDtTm` must reproduce the
spelling it was given. `IsoDate` — which is what every SEPA *payment* date is —
has no offset and keeps its full `Ord`.

### Fixed — `is_fully_accepted` accepted a report that stated nothing

0.6 made a report with no status at all not an acceptance. It did not go far
enough: "a status was reported" counted an `OrgnlPmtInfAndSts` *block*, not an
actual status, so a report carrying neither `PmtInfSts` nor `TxSts` came back
fully accepted. It now requires a real status somewhere.

### Fixed — a UTC offset beyond the `xs:dateTime` range parsed

`IsoDateTime::parse` accepted offsets up to `±23:59`. `xs:dateTime` bounds them
at `±14:00`; past that the tail is not a timezone, and reading it as one turned
malformed input into a valid timestamp. It is now rejected.

### Changed — API

- **`MsgId` is a constructor argument.** The generated default was
  `<prefix>-<epoch seconds>-<counter>`: unique within one process and worthless
  outside it, because the only property a bank's duplicate detection needs is
  surviving a restart. A value that looks like an identifier without being one is
  worse than no value. `Pain001Builder::new`, `Pain008Builder::new` and
  `Pain007Builder::new` now take it; the `.msg_id()` setters are gone.
- **The payment date is a constructor argument.**
  `CreditTransferGroup::new(name, iban, execution)` takes anything convertible
  into an `ExecutionMoment` — an `IsoDate` for an ordinary transfer, an
  `IsoDateTime` for a *terminierte Echtzeitüberweisung* — and
  `DirectDebitGroup::new(name, iban, creditor_id, collection_date)` takes an
  `IsoDate`. `.execution_date()`, `.execution_at()` and `.collection_date()` are
  gone.

  The old direct debit default was `IsoDate::today().plus_days(5)`, the SDD Core
  pre-notification floor. Which day that should be depends on the scheme, the
  sequence type, TARGET2 and the bank's cut-off — a banking-calendar question
  this crate cannot answer and should not appear to. The credit transfer default
  was "today", which is a different way of not answering it.
- `IsoDate::today()` and `IsoDateTime::now()` remain, but **nothing in the crate
  calls `today()` any more**. One implicit clock read is left: `GrpHdr/CreDtTm`,
  which `created_at()` still overrides, so a submitted file regenerates
  byte-for-byte.
- `Party` identifiers: `SchmeNm/Prtry` is validated as a `Max35Text` scheme name
  rather than as an EPC230-15 reference, so a slash in a scheme name is no
  longer rejected — and it is now transliterated on write.

### Changed — emitted output

- A `Party`'s `SchmeNm/Prtry` and a proprietary remittance `Issr` are
  transliterated under the default `CharsetPolicy`, where they were previously
  emitted verbatim. For input the crate already accepted and that was already
  inside the SEPA character set, nothing changes.
- Length limits are measured on the trimmed value. Every ISO 20022 text type is
  an `xs:string` with `whiteSpace="collapse"`, so the bank's parser strips
  padding before applying the length facet; counting it here rejected values the
  bank accepts.

### Added — tests

- **The exhaustive transliteration sweep the README already claimed.** All
  1 112 064 Unicode scalar values through both styles, asserting the output is
  SEPA-legal and non-empty. The existing full-range sweep only tested `is_sepa_char`
  *membership*; the documented claim was stronger than the test.
- Schema validation for a proprietary structured reference, against
  `pain.001.001.09` and the GBIC 5 subset — the one remittance shape whose
  `Issr` is caller text.
- Regression tests naming each defect above.

### Added — camt `Chrgs`: the return fee on a bounced collection

`Chrgs` was parsed nowhere, at either level. For SEPA the case that matters is a
**returned direct debit**: the collection comes back and the bank passes on a
return fee, which is real money the creditor is out and which is reported beside
the entry rather than inside its amount. A ledger built on this crate could not
see it.

- `CashEntry::charges` and `EntryDetail::charges`, both `Option<Charges>`.
- `Charges` carries `TtlChrgsAndTaxAmt` and `0..n` `ChargeRecord`s, with
  `total_signed_ct()` summing from the records — the level that has the
  credit/debit indicator, since the total is a magnitude with no sign.
- ISO reshaped the block: up to `.001.02` the charge sits directly under `Chrgs`
  as an `Amt`/`CdtDbtInd` pair, and from `.001.04` it moved into `Rcrd` blocks.
  Both are read, and the flat form is reported as a single record so callers
  have one shape.
- `ChargeRecord::included_in_amount` is an `Option<bool>`, not a `bool`.
  `ChrgInclInd` is optional and "the bank did not say" is a third answer:
  assuming *included* silently drops a fee, assuming *separate* silently
  double-counts one. `Charges::all_included_in_amount()` is `false` unless every
  record says so explicitly, so a caller that adds charges only when it is false
  cannot double-count on a bank that omits the flag.
- A charge with no `CdtDbtInd` is read as a debit — that is what a fee is, and
  it is the direction that cannot inflate a balance if the assumption is wrong.

### Added — a rejection now explains itself at whichever level it happened

`StsRsnInf` was parsed on a transaction and nowhere else. A bank that refuses a
**whole submission** — a duplicate `MsgId`, an unreadable document, an unknown
Creditor Identifier — sends `GrpSts = RJCT` with the reason at group level and
*no* payment-information or transaction blocks at all, so the only thing an
operator could act on was discarded. The same held for a rejected `PmtInf`,
where no transaction was reached either.

- `Pain002Document::group_reason_codes` and `group_additional_info`
  (`OrgnlGrpInfAndSts/StsRsnInf`).
- `PaymentInfoStatus::reason_codes` and `additional_info`
  (`OrgnlPmtInfAndSts/StsRsnInf`).
- `Pain002Document::reason_codes()` gathers every reason at any level, most
  general first — a rejection explains itself at exactly one level and which one
  depends on how far the bank got, so walking a single level answers the
  question only sometimes.

An unrecognised but well-formed code is carried through as `ReasonCode::Other`
rather than dropped: the reason an operator needs must not depend on whether the
enum happens to know the code. The new fixture is itself validated against
`pain.002.001.10.xsd`.

### Added — one table for every `Max*Text` bound

`validate::max_text_len(parent, element)` is the single place the ISO 20022
length limits are written down, keyed by the element **and its parent** — which
is what disambiguates the names ISO 20022 reuses: `SvcLvl/Cd` is a `Max4Text`
external code, `LclInstrm/Cd` a `Max35Text`; a bare `Id` is a container,
`Othr/Id` an identifier. `address` now derives its bounds from it instead of
holding its own constants, and consumers can pre-check their data with the same
numbers the builders enforce.

The table is enforced for **completeness**, not only for correctness: a test
walks every text node of a maximal pain.001, pain.008 and pain.007 and requires
each element to be either in the table or on an explicit list of values bounded
by their own type (`IBAN`, `BICFI`, dates, amounts, enumerated codes). An element
added to a writer with no bound fails the build. That is the other half of the
`Issr` defect class — the charset invariant was made document-wide in this same
release; this makes the length invariant document-wide too.

New `validate::MAX_CODE_LEN` (4) and `validate::MAX_BUILDING_LEN` (16).

### Added — CI gates for fuzzing and vendored data

- **The three fuzz targets run in CI** — 60 seconds each on every push, 600 on a
  weekly schedule, with crash artefacts uploaded on failure. They were only ever
  run by hand before. `build_batch` asserts an invariant rather than merely the
  absence of a panic: any batch it *accepts* must produce a document whose every
  text node is inside the SEPA character set.
- **`scripts/check-vendored-data.sh`** re-derives the SHA-256 of all ten pinned
  XSDs against the record in `tests/xsd/README.md`, and fails when a vendored
  schema has **no** recorded digest. Three of the ten had none. A schema quietly
  swapped for a defective mirror makes correct output look wrong — and the
  natural reaction is to "fix" the writer, at which point the crate emits
  genuinely invalid files with a green suite. `just verify-data` runs it
  locally and it is part of `just ci`; `just verify-tables` adds the
  reference-data conformance tests.

### Internal

- The four camt types that carry money now embed a single `ReportedAmount`
  instead of repeating five fields and an accessor with identical semantics.
  The invariant has one definition, so a new money-bearing type inherits it.

### Documentation

- The Extended Remittance Information option (EPC092-19) is named in the scope
  table. It raises `Strd` from one 140-character block to 999 × 280, but binds
  only PSPs that adhered to it separately, so sending an ERI-shaped message to
  one that did not is a rejection. The base scheme is what every SEPA PSP takes.
- `CreditTransferEntry::creditor_bic` claimed the writer emits `NOTPROVIDED`
  when it is `None`. It omits `CdtrAgt` entirely, which is what the EPC
  guidelines require and what `pain.001.003.03` makes structural; the
  `NOTPROVIDED` form belongs to the mandatory `DbtrAgt`. A test had asserted the
  correct behaviour for three releases while the documentation said the opposite.
- The README's "all 1,114,112 Unicode code points" is 1 112 064 scalar values —
  the difference is the surrogate range, which is not a `char`.
- `CreditTransferGroup::category_purpose` said setting it excluded a
  per-transaction category purpose. There is no per-transaction category purpose
  in this crate, so the conflict is not expressible.
- "Zero I/O" in the crate description is now "No I/O", with the one clock read
  named where it happens.

### Internal

- `Pain001Builder::validate` and `Pain008Builder::validate` split into
  `validate` / `validate_group` / `validate_entry`, so the two read the same way.
- `Party::write_xml_inline`, so camt.055's `Assgnr`/`Assgne`/`Cretr` reuse the
  one `PartyIdentification` writer rather than re-deriving what SEPA admits.
- The two document-wide invariant walks (charset, `Max*Text`) and both fuzz
  targets now cover camt.055 and camt.029.
- `scripts/check-vendored-data.sh` reads `camt.*` digests as well as `pain.*`.

## [0.6.0]

A correctness and standards release. Two reachable panics, three silent
mis-parses and a document-substitution hole in the XML reader are fixed;
`PstlAdr` support lands ahead of the EPC's **15 November 2026** cut-over to
structured addresses; three rules the XSD cannot express are now enforced; and a
non-standard JSON import format is removed outright. **Breaking API changes with
no deprecation path.**

> **On the cut-over date.** Version 1.0 of the 2025 SEPA rulebooks set the end
> of unstructured addresses at 22 November 2026. Version 1.1, in force since
> 5 October 2025, moved it to **15 November 2026**, to land with that year's
> Swift Standards MX release. If your notes still say the 22nd, they are a
> rulebook version behind. It is an *address* deadline and not a message-version
> one: `pain.001.001.09` and `pain.008.001.08` have been mandatory since
> 19 November 2023, and nothing on the EPC roadmap moves SEPA past them.

### Fixed — a second root element could displace the first

`Document::parse` rejected a duplicate root element, except when the first one
was self-closing: `Event::Empty` at depth 0 assigned the root directly instead
of going through the duplicate check. So `<Document/>` followed by a second
`<Document>…</Document>` handed the caller the **second** document — the exact
substitution hazard the check exists to prevent, since a validator or the next
consumer in the chain would have seen the first.

### Fixed — a `ZZ` Creditor Identifier validated

`validate_creditor_id` checked only that characters 1–2 were letters. The EPC
check digits are computed *over* the country code, so `ZZ99ZZZ…` has perfectly
consistent check digits and passed. It is now checked against ISO 3166-1
alpha-2 — the same rule `validate_bic` and `PostalAddress::new` already applied.
`CreditorIdError::InvalidCountryCode` keeps its shape; only its message changed.

### Fixed — a remittance line split across several `Ustrd` lost all but the first

`RmtInf/Ustrd` is `0..n` in camt, and German banks routinely split a long
*Verwendungszweck* into 35-character occurrences.
`EntryDetail::reference` read only the first, cutting the reference where the
invoice number usually sits. Every occurrence is now joined with a single space.

### Fixed — the default `MsgId` could collide

The generated placeholder was `sct-<epoch seconds>`. Two messages built in the
same second — the normal case in a batch job — shared a `MsgId`, which is the
key a bank de-duplicates submissions by: the second file is rejected, or
accepted and silently discarded. A process-wide counter is now appended. It is
still only a placeholder, and `msg_id()` from your own persistent sequence is
still what a production submission needs; the documentation now says so at the
setter rather than leaving it implied.

### Fixed — panics on untrusted input
- **`IsoDateTime::parse` panicked on a multi-byte time part.** The UTC offset
  was split off with `rest.split_at(rest.len() - 6)`, and for input such as
  `"2026-07-20T€€a"` — seven bytes — index 1 falls inside the first `€`, so the
  process aborted. The crate `forbid`s `unsafe_code` and lints against panics
  precisely because it parses bank-supplied text; this was a hole in that. Now
  `split_at_checked`, so the value is rejected.

### Fixed — silent mis-parses
- **`ct_from_eur_str` accepted a repeated sign.** `i64::from_str` takes a
  leading sign wherever it is handed one, so `"--5"` parsed as **+5.00 EUR**,
  `"-+5"` as −5.00 and `"+5"` as 5.00. The grammar is now exactly
  `-?[0-9]*(\.[0-9]*)?` with at least one digit.
- **`ct_from_eur_str` ignored trailing junk after the cents.** Only the first
  two fractional characters were checked, so `"1.50abc"` parsed as 1.50 EUR.
  The whole fractional part is now validated.
- **A batch booking of one transaction read as an ordinary payment.**
  `CashEntry::batch_booked` was derived from `TxDtls` count > 1, ignoring the
  `NtryDtls/Btch` element — the bank's own assertion that a booking is an
  aggregate. Worse, the entry total was then attributed to the single itemised
  detail even when `Btch/NbOfTxs` said the bank had aggregated more, which
  understates a collection run. Both now follow `Btch`.

### Fixed — data silently dropped or invented when reading bank files
- **pain.002 invented values for fields a bank may legally omit.**
  `OrgnlEndToEndId` and `TxSts` are both `0..1` in every version (verified
  against the published `pain.002.001.15` schema), yet a missing reference was
  filled in as `"NOTPROVIDED"` and a missing status as `Other("UNKNOWN")`.
  Both are strings a bank can also send for real, so a caller could not tell an
  unattributable rejection from an attributed one. They are now `Option`s, as is
  `OrgnlPmtInfId`.
- **`Pain002Document::is_fully_accepted` ignored non-rejection statuses.** It
  asked only whether anything was *rejected*, so a `PmtInfSts` of `PDNG` or
  `PART` under an accepted `GrpSts` counted as fully accepted. It now requires
  every status that was reported, at all three levels, to be an accepted one —
  and a report stating no status at all is no longer an acceptance.
- **A non-IBAN account read as an empty string.** `Acct/Id` is a choice between
  an `IBAN` and a proprietary `Othr/Id`, and camt.052/053/054 collapsed it to
  one `String`, making an account that is not IBAN-addressable indistinguishable
  from an account with no identifier.
- **`Ntry/AddtlNtryInf` and `TxDtls/AddtlTxInf` were dropped entirely.** These
  carry the bank's own statement text; for an entry with no `NtryDtls` the
  former is often the only remittance information in the file.
- **`RtrInf/AddtlInf` was dropped**, so a return carried its reason code but not
  the bank's explanation of it.
- **pain.002 lost every party name in a current-version report.** `Dbtr` and
  `Cdtr` are a `Party40Choice` from `.001.10`, so the name is `Cdtr/Pty/Nm`; the
  parser read only the flat `.001.03` form and returned `None`.
- **Only the first `StsRsnInf/AddtlInf` was kept.** It is
  `maxOccurs="unbounded"`, and banks use that — a legal notice spans lines, and
  a VoP close-match name over 105 characters arrives split in two.
  `TransactionStatus::additional_info` is now a `Vec<String>`.
- **The EPC conversion table rewrote U+0020 SPACE to `"."`.** Space is
  SEPA-legal and the row should never have been generated. Unreachable in
  practice — `transliterate` short-circuits on `is_sepa_char` before consulting
  the table — but it is wrong data, and three new invariant tests now assert the
  table contains no legal source characters, no illegal replacements and no
  empty ones.

### Fixed — documentation
- **The transliteration docs claimed 26 multi-character romanisations; there
  are 20** — six Greek and fourteen Cyrillic. The number was wrong in the module
  docs, the `Transliteration::Epc` docs and the README, and is now pinned by a
  test.

### Added — pain.007 SEPA Direct Debit reversal
- **New [`pain007`] module.** A reversal is the creditor sending a settled
  collection back — the counterpart to a debtor-initiated refund (camt.054) and
  a reject (pain.002), and the last message-level gap in the SDD lifecycle.
- `ReversalEntry::reverse(&group, &entry, reason)` builds the `OrgnlTxRef` from
  the `DirectDebitGroup` and `DirectDebitEntry` that produced the collection, so
  the reversal cannot disagree with what was sent. `ReversalEntry::new` covers
  the case where only stored data is available.
- `OriginalCollection` is **required**, not optional. Plain ISO permits a
  reversal carrying references only; the DK technical validation subset makes
  `OrgnlTxRef` and the mandate inside it mandatory, so the references-only form
  is not one a German bank accepts.
- Its fields are set in pairs (`payment_type`, `debtor`, `creditor`) because the
  subset makes `SvcLvl`/`LclInstrm`/`SeqTp` and party name/account
  all-or-nothing — a half-filled block is unconstructible rather than
  schema-invalid.
- Reversing more than was collected is rejected.
- Validated in CI against `pain.007.001.09`.

### Added — Verification of Payee (pain.002)
VoP has been mandatory for euro credit transfers since 9 October 2025, and its
results arrive inside the pain.002. The parser did not know the codes, so every
outcome fell through to `Other` and the file read as an unrecognised status
report.

- `PaymentStatus` gained `Rcvc`, `Rvmc`, `Rvnm`, `Rvna` and the group-level
  `Rvcm`, with `verification()` returning a typed `VerificationOutcome` and
  `is_verification()` separating that axis from acceptance. A verification
  status is deliberately **not** an acceptance: `RCVC` says a name matched,
  which is a different question from whether the payment was taken.
- `StatusCount` (`NbOfTxsPerSts`) at group and payment-information level — a VoP
  report on 462 payments states counts per outcome and itemises only the ones
  needing attention, and those counts were previously discarded.
- Tested against the Deutsche Kreditwirtschaft's own published VoP example, and
  the fixture is itself validated against `pain.002.001.10`.

### Added — postal addresses
- **New [`address`] module and `PostalAddress` type**, covering the elements
  common to `PostalAddress6` (pain.001.001.03 / pain.008.001.02) and
  `PostalAddress24` (pain.001.001.09 / pain.008.001.08): `Dept`, `SubDept`,
  `StrtNm`, `BldgNb`, `PstCd`, `TwnNm`, `CtrySubDvsn`, `Ctry` and `AdrLine`.
  One address value therefore validates against every ISO schema this crate
  emits — checked against all four XSDs in CI.
- **The unstructured form is unrepresentable.** From 15 November 2026 the EPC
  schemes reject an address without `TwnNm` and `Ctry`.
  `PostalAddress::new` takes both, so only the structured and hybrid forms are
  constructible — the same treatment `Iban` and `IsoDate` already get.
  `AddressFormat` reports which of the two an address is.
- `Ctry` is checked against the ISO 3166 table rather than the XSD's `[A-Z]{2}`,
  so `ZZ` is refused at construction.
- Wired into both builders: `CreditTransferGroup::debtor_address`,
  `CreditTransferEntry::with_creditor_address`,
  `DirectDebitGroup::creditor_address`,
  `DirectDebitEntry::with_debtor_address`. The address is written after `Nm`,
  which is where `PartyIdentification`'s `xs:sequence` puts it.
- The legacy DK schemas have no structured address type — their
  `PostalAddressSEPA` holds only `Ctry` and two `AdrLine`s — so they refuse with
  `ValidationError::UnsupportedBySchema` rather than emitting something their
  own XSD rejects. `CreditTransferSchema::supports_postal_address` and
  `DirectDebitSchema::supports_postal_address` expose the capability.

### Added — scheduled instant transfers
- `CreditTransferGroup::execution_at` and `ExecutionMoment` emit
  `ReqdExctnDt/DtTm`, the DK's *terminierte Echtzeitüberweisung*. Only
  `pain.001.001.09` types `ReqdExctnDt` as a date/time choice, so a timed
  execution on an older schema is rejected rather than quietly reduced to the
  day — a payment meant to leave at 11:00 must not silently become "some time
  that day". `CreditTransferSchema::supports_execution_time` exposes the
  capability.
- Two rules travel with it that no XSD can express, both enforced by `build()`:
  the group must be `LclInstrm = INST`, and the timestamp must carry a UTC
  offset. See *Changed — emitted output*.

### Added — camt
- **`AccountRef`** replaces the flat `account_iban` / `account_servicer_bic`
  pair on `Camt052Report`, `Camt053Statement` and `Camt054Notification`. It
  carries `iban`, `other_id`, `currency` and `servicer_bic`, with `any_id()` as
  the display shortcut.
- `CashEntry::additional_info` (`AddtlNtryInf`),
  `EntryDetail::additional_info` (`AddtlTxInf`) and
  `EntryDetail::return_additional_info` (`RtrInf/AddtlInf`).
- **`CashEntry::batch`** exposes the `NtryDtls/Btch` block as a new `BatchInfo`
  type: `MsgId`, `PmtInfId` and `NbOfTxs`. `Btch/PmtInfId` is the element that
  matches a booked collection back to the `PmtInf` group you submitted, without
  guessing from amounts and dates.

### Added — pain.002
- `TransactionStatus::original_instruction_id` (`OrgnlInstrId`) — the
  alternative key a bank may echo when it omits `OrgnlEndToEndId`.
- `TransactionStatus::is_rejected()`, which treats an unreported status as
  "not a rejection" rather than assuming one.
- `ns::PAIN002_001_10`, the version the EPC 2025 Customer-to-PSP guidelines
  specify as the reply to a `pain.001.001.09` / `pain.008.001.08` submission.
  The module documented only the 2009-era namespaces.

### Added — validation
- **A mandate amendment can no longer both state and suppress the previous
  debtor account.** `OrgnlDbtrAcct` occurs once, and setting
  `same_mandate_new_account` alongside `original_debtor_iban` used to silently
  drop the IBAN. It is now `ValidationError::MutuallyExclusive`.
- `ValidationError::TooMany` for repeating elements over their cap — currently
  `PstlAdr/AdrLine`, which the EPC limits to two where the XSD permits seven.
- `validate::check_text` for element-specific `Max*Text` bounds.

### Changed — schema validation
- **Generated SCT and SDD documents are now validated against the Deutsche
  Kreditwirtschaft's GBIC 5 technical validation subsets as well as the ISO
  schemas.** A subset is a restriction of the ISO schema down to what German
  banks accept, so passing it is the harder test; it is what established that
  `OrgnlTxRef` is mandatory on a reversal, where ISO leaves it optional.
- `pain.007.001.09.xsd` and `pain.002.001.10.xsd` are vendored from the DK's
  freely published Anlage 3 package — authoritative copies, unlike the
  third-party mirrors the older ISO schemas came from. `tests/xsd/README.md`
  now separates the two provenance classes and records SHA-256 for each.

### Removed — `parse_simple_json`, the `json` feature, `serde_json`

`camt054::parse_simple_json` read a flat `{iban, amount_eur, date}` record out
of "a bank's CSV or JSON export". No such format is specified anywhere: it was
an invented shape living inside a module named after an ISO 20022 message, with
its own entry type (`Camt054Entry`) whose `to_ledger_ct` used the *opposite*
sign convention to `CashEntry::signed_ct` beside it. Two sign conventions in one
crate is a defect waiting to be someone's rounding error.

Gone, with `Camt054Entry`, `ReturnInfo`, `SimpleJsonError`, the `json` feature
and the optional `serde_json` dependency. Parse your export with `serde` and
hand the fields to `ct_from_eur_str` and `IsoDate::parse`, which is all the
removed code did.

### Internal

- `validate_iban` folds mod-97 in one pass over the rotated bytes instead of
  building a rearranged string and an expanded decimal string. Same result,
  two allocations fewer per IBAN.
- The `Cow`-and-`unwrap_or` dance at every write site is replaced by one
  infallible `CharsetPolicy::render`, so "serialisation runs after validation"
  is stated once rather than re-derived eight times.

### Changed — API
- **`bic::is_bic_country_code` is gone.** The ISO 3166 table now lives in the
  new [`country`] module as `country::is_country_code`, because `PstlAdr/Ctry`
  needs the same answer as a BIC's characters 5–6. Re-exported at the crate
  root; the old name was never accurate about its scope.
- **`Camt053ParseError::MissingElement` and `Pain002ParseError::InvalidAmount`
  are gone.** Neither was ever constructed. Both enums are `#[non_exhaustive]`,
  so a `match` with a wildcard arm is unaffected.
- **`CreditorId` gained the conversions `Iban` and `Bic` already had**:
  `Ord`/`PartialOrd`, `Deref<Target = str>`, `Borrow<str>` and
  `TryFrom<String>`. A `HashMap<CreditorId, _>` can now be looked up with a
  `&str`.
- `CashEntry` gained a field; it is `#[non_exhaustive]`, so construction was
  already through the parsers.
- `RemittanceInfo::validate` takes a `CharsetPolicy` — lengths are measured
  after transliteration.
- `validate::truncate_chars` returns `&str` rather than a `Cow` that was always
  `Borrowed`.
- `validate_bic` strips whitespace before validating, so `"COBA DE FF XXX"` is
  accepted like a spaced IBAN already was.
- New `ValidationError` variants: `Requires`, `Duplicate`.

### Changed — emitted output

- **`ReqdExctnDt/DtTm` now requires `LclInstrm = INST` and a UTC offset.** The
  DK validation subset annotates `DtTm` *"Only allowed for SCTinst"*, with the
  usage rule *"Only UTC time format or local time with UTC offset format can be
  used"*. Neither is expressible in XSD, so a file breaking them validates
  cleanly under `xmllint` and is rejected on ingestion. Both are now
  `ValidationError::Requires`. A timed execution built without
  `.local_instrument(LocalInstrument::Inst)`, or from a timestamp with no `Z`
  or `±hh:mm`, no longer builds.
- **A duplicate `PmtInfId` across groups is rejected.** It is the key a bank
  echoes back in `pain.002` and in the `NtryDtls/Btch` block of a camt
  statement, so two groups sharing one make a booking unattributable. New
  `ValidationError::Duplicate`.
- **`RmtInf/Ustrd` is no longer silently truncated.** The 140-character limit
  was checked on the input and the writer then cut the *transliterated* value,
  so 140 German characters became 141 and lost their tail — where an invoice
  number tends to sit. The limit now binds on the transliterated text and an
  over-long line is a `ValidationError::TooLong`.
- **`CreditTransferGroup`'s default execution date is today**, not five days
  out. The old value was `pain.008`'s pre-notification floor borrowed wholesale;
  a credit transfer has no pre-notification period, so the default meant "five
  days from now" for no reason anyone could state. Set
  `.execution_date(…)` explicitly, as every real caller does.
- New address elements appear only when an address is set. Nothing else changes
  the bytes emitted for an input that 0.5 accepted.

### Internal

- The four camt types that carry money now embed a single `ReportedAmount`
  instead of repeating five fields and an accessor with identical semantics.
  The invariant has one definition, so a new money-bearing type inherits it.

### Documentation

- SWIFT IBAN Registry citation moved to release 102 (June 2026). The 89
  structures are unchanged.
- The site's validation guide gains the length-after-transliteration rule and
  the `PmtInfId` uniqueness rule; the credit-transfer guide gains the two timed
  execution rules.

[`address`]: https://docs.rs/sepa/latest/sepa/address/
[`country`]: https://docs.rs/sepa/latest/sepa/country/

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
