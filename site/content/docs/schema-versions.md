+++
title = "Schema versions"
description = "Supported ISO 20022 versions and per-message schema selection."
weight = 11
+++

The required version varies by bank, so it is chosen per message rather than
at compile time.

**The version is not the scheme.** SCT, SCT Instant and OCT Inst all use
`pain.001.001.09`. Pick the *version* with `Pain001Builder::schema` and the
*scheme* with `CreditTransferGroup::kind`; see
[credit transfers](/docs/credit-transfers/). `EOLO` (OCT Inst) requires
`pain.001.001.09`.

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
    let builder = Pain008Builder::new(
        "Stadtwerke GmbH",
        "DD-2026-07-001",
        "2026-07-15T09:00:00".parse()?,
    ).schema(schema);

    assert_eq!(schema.message_id(), "pain.008.001.02");
    assert_eq!(builder.group_count(), 0);
    Ok(())
}
```

The versions differ on the wire, not just in the namespace: ISO renamed `BIC`
to `BICFI` in the 2019 maintenance release, `ReqdExctnDt` became a date or
date-time choice, and the legacy German schema places the `SMNDA` marker in a
different element. The builders emit the right form for the selected version.
A feature the version lacks — such as an instant transfer on a schema with no
local-instrument element — is a typed error.

## Why not newer ISO versions?

ISO has published `pain.001.001.13`, `pain.008.001.12` and `pain.002.001.15`,
but a newer ISO version is not an accepted SEPA version. **Use the version
required by the applicable scheme profile and your bank.**

| Message | Vendored newer ISO version | Supported current SEPA version |
|---|---|---|
| `pain.001` | `.001.13` | **`.001.09`** |
| `pain.008` | `.001.12` | **`.001.08`** |
| `pain.002` | `.001.15` | **`.001.10`** |
| `pain.007` | `.001.13` | **`.001.09`** |

The supported SEPA versions have been mandatory since **19 November 2023**.
Address requirements are independent of the version; see
[postal addresses](/docs/addresses/).

## How the output is checked

CI validates generated fixtures for each emitted version against pinned ISO
XSDs, plus the German GBIC subsets for the default SCT/SDD versions. Builders
check field rules; they do not run an XSD validator per output, and schema
validity does not guarantee bank acceptance.
