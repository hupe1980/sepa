+++
title = "Status reports & Verification of Payee"
description = "Parse pain.002 payment status reports in Rust, including Verification of Payee outcomes — match, close match, no match — which have been mandatory for euro credit transfers since 9 October 2025."
weight = 5
+++

`pain.002` is the bank's answer to a file you submitted. The parser is
namespace- and version-agnostic, so `pain.002.001.10` (what the current EPC
guidelines specify), the older `.001.03`, and the German variants are all read
by the same code.

## Acceptance and rejection

```rust,no_run
use sepa::parse_pain002;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("status-report.xml")?;
    let doc = parse_pain002(&xml)?;

    if doc.is_fully_accepted() {
        println!("accepted");
    } else {
        for tx in doc.rejected_transactions() {
            // `OrgnlEndToEndId` is optional in every version, so this is an
            // Option rather than a stand-in you cannot tell apart from real data.
            match tx.original_end_to_end_id.as_deref() {
                Some(id) => println!("{id} rejected: {:?}", tx.reason_codes),
                None => println!("unattributable rejection: {:?}", tx.reason_codes),
            }
        }
    }
    Ok(())
}
```

`ACTC` means the file was technically valid, not that money moved. `ACSC` is
settlement completed. `PaymentStatus::is_final` tells the two apart when you
need to wait for a terminal state.

## Verification of Payee

Since **9 October 2025** the payer's bank must check the payee's name against
the account before executing a credit transfer, and it reports the result in
the `pain.002`. A status report is therefore no longer only about acceptance.

```rust,no_run
use sepa::{VerificationOutcome, parse_pain002};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("status-report.xml")?;

    for block in &parse_pain002(&xml)?.payment_info_statuses {
        // A file of hundreds reports counts per outcome and itemises only
        // what needs a decision.
        for count in &block.status_counts {
            println!("{} x {}", count.count, count.status);
        }

        for tx in &block.transactions {
            match tx.status.as_ref().and_then(|s| s.verification()) {
                Some(VerificationOutcome::CloseMatch) => {
                    // The payee's *actual* name comes back, to show the payer.
                    println!("close match, bank holds: {:?}", tx.additional_info);
                }
                Some(VerificationOutcome::NoMatch) => {
                    println!("no match — executing anyway shifts liability to the payer");
                }
                Some(VerificationOutcome::NotApplicable) => {
                    println!("no answer from the payee's bank");
                }
                Some(VerificationOutcome::Match) | None => {}
            }
        }
    }
    Ok(())
}
```

| Code | Outcome | What to do |
|---|---|---|
| `RCVC` | Match | Nothing |
| `RVMC` | Close match | Show the returned name and let the payer confirm |
| `RVNM` | No match | Warn clearly; proceeding moves liability to the payer |
| `RVNA` | Not applicable | No answer, a timeout, or a bank outside the scheme |

> A verification status is deliberately **not** an acceptance. `RCVC` says a
> name matched, which is a different question from whether the payment was
> taken — so `is_accepted()` stays `false` for it, and `verification()` answers
> the other question.

## Reason codes

Rejections carry an ISO reason code: `AC01` wrong account, `AC04` closed,
`AC06` blocked, `AM04` insufficient funds, `MD01` no valid mandate, `MD06`
debtor revoked. Unknown codes are carried through rather than dropped, so a
bank-specific value still reaches your logs.
