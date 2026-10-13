+++
title = "Schema versions"
description = "Supported ISO 20022 versions and per-message schema selection."
weight = 11
+++

Which version a bank requires varies by bank and by regulatory cut-over, so it
is a per-message choice rather than a compile-time constant.

**The version is not the scheme.** These are two independent axes and it is
worth keeping them apart: all three credit-transfer schemes — SCT, SCT Instant
and OCT Inst — use `pain.001.001.09` in the supported current profiles. Pick the *version* with
`Pain001Builder::schema` and the *scheme* with `CreditTransferGroup::kind`; see
[credit transfers](/docs/credit-transfers/). The one interaction is that OCT
Inst postdates the older schemas, so `EOLO` requires `pain.001.001.09`.

| Message | Versions, default first |
|---|---|
| `pain.001` | `.001.09` · `.001.03` · `.003.03` (legacy German, end-of-life) |
| `pain.008` | `.001.08` · `.001.02` · `.003.02` (legacy German, end-of-life) |
| `pain.007` | `.001.09` — the only version SEPA defines |
| `pain.002` | parses `.001.10`, `.001.03`, `.002.03`, `.003.03` |
| `camt.055` / `camt.029` | `.001.05` / `.001.06` — the pair the DFÜ-Abkommen names |
| `camt.052/053` | parses `.001.02`, `.001.06`, `.001.08` |
| `camt.054` | parses `.001.08` |

Readers require the exact supported message namespace and envelope. The
[reader contract](/docs/reader-contracts/) distinguishes accepted versions from
versions covered by pinned XSD fixtures; neither implies full inbound XSD validation.

## Selecting one

Both builder enums parse from the message identifier *and* the namespace URN,
so the target can come from configuration rather than a recompile:

```rust
use sepa::Pain008Builder;
use sepa::pain008::DirectDebitSchema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema: DirectDebitSchema = "pain.008.001.02".parse()?;
    let builder = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001").schema(schema);

    assert_eq!(schema.message_id(), "pain.008.001.02");
    assert_eq!(builder.group_count(), 0);
    Ok(())
}
```

The versions differ on the wire, not just in the namespace, and the crate
handles the differences rather than papering over them: `BIC` was renamed
`BICFI` in the 2019 maintenance release, `ReqdExctnDt` became a date-time
choice, and the `SMNDA` marker sits in a different element in the legacy German
schema. Where a feature genuinely does not exist in a version — an instant
transfer on a schema with no local-instrument element — you get a typed error
instead of a file that fails its own schema.

## Why not newer ISO versions?

ISO advises communities to use the most recent message definition available,
and has published `pain.001.001.13`, `pain.008.001.12` and `pain.002.001.15`.

**Use the version required by the applicable scheme profile and your bank.**
A newer ISO version is not automatically an accepted SEPA version.

| Message | Vendored newer ISO version | Supported current SEPA version |
|---|---|---|
| `pain.001` | `.001.13` | **`.001.09`** |
| `pain.008` | `.001.12` | **`.001.08`** |
| `pain.002` | `.001.15` | **`.001.10`** |
| `pain.007` | `.001.13` | **`.001.09`** |

Those versions have been mandatory since **19 November 2023**. ISO's guidance is
written for communities free to choose their own version; a scheme participant
is not one.

Address requirements are independent of message-version selection; see
[postal addresses](/docs/addresses/). Proposals for future versions do not change
the versions accepted by the current profiles.

## How the output is checked

CI validates generated fixtures for each emitted schema version against pinned
ISO XSDs, plus German GBIC subsets for the default SCT/SDD versions. Builders
validate field rules; they do not run an XSD validator for each output. Schema
validity is test evidence, not a guarantee of bank acceptance.
