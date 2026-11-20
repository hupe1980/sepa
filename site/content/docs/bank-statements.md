+++
title = "Bank statements"
description = "Parse camt.052, camt.053 and camt.054 statements in Rust: batch bookings, matching a collection run back to your file, returns and return fees."
weight = 8
+++

The three camt messages report account movements and differ mainly in timing.
They share one entry model, so reconciliation code works across all of them.

| Message | Content | When |
|---|---|---|
| `camt.052` | Intraday report, **provisional** | During the day |
| `camt.053` | Account statement; inspect each reported entry status | Commonly overnight |
| `camt.054` | Notification of specific debits and credits | As events happen |

Accepted versions are camt.052/camt.053 001.02, 001.06 and 001.08, and camt.054
001.08; see [reader contracts](@/docs/reader-contracts.md) for limits. Parsing
is not XSD validation.

## Reading a statement

```rust,no_run
use sepa::parse_camt053;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("statement.xml")?;
    let doc = parse_camt053(&xml)?;
    let stmt = &doc.statements[0];

    // `Acct/Id` is a choice — an IBAN or a proprietary identifier — so both
    // are exposed, exactly as reported, and `any_id()` is the shortcut.
    // `{:?}` prints `AccountId(DE…3000)`: logs never carry the full account.
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

Amounts are magnitudes with a separate credit/debit indicator. `signed_ct()`
returns signed hundredths only when magnitude, currency and direction are usable
and consistent; otherwise it returns `None`, and the raw values remain on the
entry for escalation.

`reference()` is the remittance information (*Verwendungszweck*). German banks
often split it across repeated `RmtInf/Ustrd` elements of 35 characters, so all
occurrences are joined with a single space.

## Batch bookings

A direct debit run is normally booked as **one** aggregate entry with the
transactions itemised underneath. The `Btch` block names the `PmtInfId` of the
group that produced it, which matches the entry back to your file. Check that
the parts reconcile before posting:

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

            // Unresolved or inconsistent parts cannot be reconciled.
            if !entry.details_reconcile() {
                eprintln!("batch does not reconcile — do not post");
            }
        }
    }
    Ok(())
}
```

`signed_ct()` on a detail returns `None` when the statement does not determine
that transaction's amount. Do not substitute the entry total.

## Returns

camt.054 reports returned collections:

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

Return fees are reported in `Chrgs`, on the entry or on the transaction detail
depending on the bank, so read both:

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

`ChrgInclInd` is optional, so `included_in_amount` is an `Option<bool>` and
`all_included_in_amount()` is `false` unless **every** record says so. `false`
does not authorize posting: inspect each record, where `None` is unresolved.
Missing direction is not assumed to be debit. Absent records, mixed currencies
and conflicting totals produce no trusted sum; invalid totals remain as raw text.

In `.001.02` the charge sits directly under `Chrgs`; later versions use `Rcrd`
blocks. Both are read, and the flat form is reported as a single record.

## Dates and raw values

ISO types a booking date as a date *or* a date-time. Both are parsed, and the
original text is kept alongside:

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

The raw text remains when typed conversion fails; XML, identity and limit
violations still reject the input.

## Free text

`AddtlNtryInf` (the bank's statement text, often the only description of an
entry without details) is exposed on the entry, and `AddtlTxInf` per detail.

## Security

Parsing uses [`quick-xml`](https://crates.io/crates/quick-xml): comments,
entity references, CDATA and prefixed namespaces (`<ns2:Document>`) are handled.
`<!DOCTYPE>` is rejected, nesting is capped, and a second root element is an
error.
