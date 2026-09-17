+++
title = "Bank statements"
description = "Parse camt.052, camt.053 and camt.054 statements in Rust: batch bookings, matching a collection run back to your file, returns and return fees."
weight = 8
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

    if let Some(ct) = stmt.closing_balance().and_then(|b| b.signed_ct()) {
        println!("closing {ct} ct");
    }

    for entry in &stmt.entries {
        match entry.signed_ct() {
            Some(ct) => println!("{ct:+} ct {}", entry.reference().unwrap_or("")),
            // The statement did not determine this one. Escalate the row —
            // never substitute a figure.
            None => println!("unresolved: amount {:?} direction {:?}",
                             entry.amount.amount_raw, entry.amount.direction_raw),
        }
    }
    Ok(())
}
```

Amounts are always positive in the file with a separate credit/debit indicator.
`signed_ct()` gives the ledger amount: credit positive, debit negative.

**It returns `Option`, and the `None` is load-bearing.** A statement can fail to
give you a ledger figure two ways: an `Amt` this crate cannot represent — more
than two significant decimals, or a magnitude past `i64` ct — or a `CdtDbtInd`
that is absent or unrecognised. Neither is defaulted, because an unknown
direction taken for a credit turns a EUR 1,000 debit into a EUR 1,000 credit.

The entry is reported either way. `entry.amount` holds exactly what the bank
sent, so you can log what you could not read, and **a booking is never dropped**
for being unreadable — a missing booking looks identical to one that never
happened.

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

### The return fee

A return costs the creditor money, and the fee is reported in `Chrgs` rather
than inside the entry amount. It appears on the entry or on the transaction
detail depending on the bank, so read both:

```rust
use sepa::parse_camt053;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Ntry><Amt Ccy="EUR">75.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
      <Chrgs><Rcrd><Amt Ccy="EUR">3.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
             <ChrgInclInd>false</ChrgInclInd></Rcrd></Chrgs>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#;

    let entry = &parse_camt053(xml)?.statements[0].entries[0];
    if let Some(charges) = &entry.charges {
        // Negative: a fee is money out.
        assert_eq!(charges.total_signed_ct(), Some(-300));
        // Stated as *not* already inside the entry amount, so a ledger posts it
        // in addition to the 75.00.
        assert!(!charges.all_included_in_amount());
    }
    Ok(())
}
```

`ChrgInclInd` is optional, and "the bank did not say" is a third answer rather
than a default. `included_in_amount` is therefore an `Option<bool>`, and
`all_included_in_amount()` is `false` unless **every** record says so
explicitly — so a caller that adds charges only when it is false cannot
double-count against a bank that omits the flag. A charge with no `CdtDbtInd` is
read as a debit: that is what a fee is, and it is the direction that cannot
inflate a balance if the assumption is wrong.

ISO reshaped this block across versions — up to `.001.02` the charge sits
directly under `Chrgs`, and from `.001.04` it moved into `Rcrd` blocks. Both are
read, and the flat form is reported as a single record so there is one shape to
handle.

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
