+++
title = "Schema versions"
description = "Which ISO 20022 message version to send to a SEPA bank, why the newest ISO version is the wrong choice, and how to select a version from configuration in Rust."
weight = 9
+++

Which version a bank requires varies by bank and by regulatory cut-over, so it
is a per-message choice rather than a compile-time constant.

| Message | Versions, default first |
|---|---|
| `pain.001` | `.001.09` · `.001.03` · `.003.03` (legacy German, end-of-life) |
| `pain.008` | `.001.08` · `.001.02` · `.003.02` (legacy German, end-of-life) |
| `pain.007` | `.001.09` — the only version SEPA defines |
| `pain.002` | parses `.001.10`, `.001.03` and the German variants |
| `camt.052/053/054` | parses `.001.02` through `.001.13` |

## Selecting one

Both builder enums parse from the message identifier *and* the namespace URN,
so the target can come from configuration rather than a recompile:

```rust
use sepa::Pain008Builder;
use sepa::pain008::DirectDebitSchema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema: DirectDebitSchema = "pain.008.001.02".parse()?;
    let builder = Pain008Builder::new("Stadtwerke GmbH").schema(schema);

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

## Why not ISO's newest version?

ISO advises communities to use the most recent message definition available,
and has published `pain.001.001.13`, `pain.008.001.12` and `pain.002.001.15`.

**For SEPA that advice does not apply.** The version is fixed by the scheme
rulebook, and sending `pain.001.001.13` to a SEPA bank gets it rejected.

| Message | ISO's newest | What SEPA mandates |
|---|---|---|
| `pain.001` | `.001.13` | **`.001.09`** |
| `pain.008` | `.001.12` | **`.001.08`** |
| `pain.002` | `.001.15` | **`.001.10`** |
| `pain.007` | `.001.13` | **`.001.09`** |

Those versions have been mandatory since **19 November 2023**, and nothing on
the EPC's published roadmap moves SEPA past them. ISO's guidance is written for
communities free to choose their own version; a scheme participant is not one.

The other date you will hear about — 15 November 2026 — is a different rule
entirely. It is the day unstructured addresses stop being accepted, not a
change of message version. See [postal addresses](/docs/addresses/).

## How the output is checked

Every generated document is validated in continuous integration against the
real ISO schema for its version — and the two default versions are additionally
validated against the stricter German validation subsets, which restrict the
ISO schema down to what those banks actually accept.

Passing both is a stronger guarantee than passing either. It is also how the
crate established that a reversal's original-transaction reference is
mandatory in practice even though ISO marks it optional.
