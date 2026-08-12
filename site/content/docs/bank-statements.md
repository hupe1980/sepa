+++
title = "Bank statements"
description = "Parse camt.052, camt.053 and camt.054 bank statements in Rust. Covers batch bookings, reconciling a collection run against your own file, returns, and typed booking dates."
weight = 6
+++

The three camt messages describe the same thing — movements on an account — and
differ mainly in timing. They share one entry model, so reconciliation code
works across all of them.

| Message | Content | When |
|---|---|---|
| `camt.052` | Intraday report, **provisional** | During the day |
| `camt.053` | End-of-day statement, booked and final | Overnight |
| `camt.054` | Notification of specific debits and credits | As events happen |

Every ISO version from `.001.02` to `.001.13` is accepted, including the `.07`
reshaping of `Ntry/Sts` into a code choice and of parties into a `Pty` wrapper.

## Reading a statement

```rust,no_run
use sepa::parse_camt053;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("statement.xml")?;
    let doc = parse_camt053(&xml)?;
    let stmt = &doc.statements[0];

    // `Acct/Id` is a choice — an IBAN or a proprietary identifier — so both
    // are exposed and `any_id()` is the display shortcut.
    println!("{:?} {:?}", stmt.account.any_id(), stmt.account.currency);

    if let Some(closing) = stmt.closing_balance() {
        println!("closing {} ct", closing.signed_ct());
    }

    for entry in &stmt.entries {
        println!("{:+} ct {}", entry.signed_ct(), entry.reference().unwrap_or(""));
    }
    Ok(())
}
```

Amounts are always positive in the file with a separate credit/debit indicator.
`signed_ct()` gives the ledger amount: credit positive, debit negative.

`reference()` is the remittance information — the *Verwendungszweck*. ISO makes
`RmtInf/Ustrd` repeatable, and German banks routinely split a long reference
into 35-character chunks, so every occurrence is joined with a single space.
Reading only the first would cut the reference exactly where the invoice number
tends to sit.

## Batch bookings

A direct debit run is normally booked as **one** aggregate entry with the
individual transactions itemised underneath. Two things matter here.

First, the `Btch` block names the `PmtInfId` of the group that produced the
booking — that is how a statement entry is matched back to the file you sent,
without guessing from amounts and dates.

Second, the parts have to account for the whole before you post anything.

```rust,no_run
use sepa::parse_camt053;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("statement.xml")?;

    for stmt in &parse_camt053(&xml)?.statements {
        for entry in &stmt.entries {
            if !entry.batch_booked {
                continue;
            }

            if let Some(batch) = &entry.batch {
                println!("from our group {:?}", batch.payment_info_id);
            }

            for detail in &entry.details {
                // Resolved from the detail, from AmtDtls, or from the entry
                // when there is only one transaction to attribute it to.
                println!("  {:?} {:?}", detail.end_to_end_id, detail.signed_ct());
            }

            // False means the statement's parts do not add up to its whole.
            // Escalate rather than post.
            if !entry.details_reconcile() {
                eprintln!("batch does not reconcile — do not post");
            }
        }
    }
    Ok(())
}
```

> `signed_ct()` on a detail returns `Option`. `None` means the statement does
> not determine that transaction's amount — which is exactly the case where the
> tempting workaround, reusing the entry total for each detail, multiplies a
> batch booking by its transaction count.

## Returns

A returned collection is where camt.054 earns its place:

```rust,no_run
use sepa::parse_camt054;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("notification.xml")?;

    for notification in &parse_camt054(&xml)?.notifications {
        for returned in notification.returns() {
            let detail = returned.first_detail();
            println!(
                "{:?}: {:?}",
                returned.return_reason_code(),
                detail.and_then(|d| d.return_additional_info.as_deref()),
            );
        }
    }
    Ok(())
}
```

`is_return()` on an entry checks **every** detail, so a single returned
collection inside an otherwise-fine batch is still reported.

## Dates and raw values

ISO types a booking date as a date *or* a date-time, so the same field arrives
as `2026-07-20` from one bank and `2026-07-20T09:14:00` from the next. Both are
handled, and the original text is kept alongside the parsed value:

```rust,no_run
use sepa::parse_camt053;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("statement.xml")?;

    for stmt in &parse_camt053(&xml)?.statements {
        for entry in &stmt.entries {
            println!("{:?} (as sent: {:?})", entry.booking_date(), entry.booking_date_raw);
        }
    }
    Ok(())
}
```

Nothing is discarded because it did not fit the expected shape, so a
non-conforming file stays readable rather than being rejected wholesale.

## Free text

`AddtlNtryInf` carries the bank's own statement text, and for an entry with no
itemised detail it is often the only description in the file. It is exposed on
the entry, alongside `AddtlTxInf` per detail.

## Security

Parsing uses [`quick-xml`](https://crates.io/crates/quick-xml), so comments,
entity references, CDATA and self-closing tags are all handled correctly, and
a prefixed `<ns2:Document>` parses identically to a default-namespaced one.
`<!DOCTYPE>` is rejected outright, nesting is capped, and a second root element
is an error rather than silently replacing the first.
