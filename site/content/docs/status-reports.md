+++
title = "Status reports & Verification of Payee"
description = "Parse pain.002 status reports in Rust: Verification of Payee outcomes, and why a rejection explains itself at only one of three levels."
weight = 7
+++

`pain.002` requires strict resolved identity and supports 001.10, 001.03, 002.03
and 003.03. Identity is not full XSD validation. See
[reader contracts](@/docs/reader-contracts.md) for finite limits and evidence boundaries.

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

## Inspect Every Reported Level

Reasons can appear at group, payment-information or transaction level.
`rejected_transactions()` only covers transaction rows; `reason_codes()` gathers
reasons from all three levels:

```rust
use sepa::parse_pain002;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
 <CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr>
  <OrgnlGrpInfAndSts>
    <OrgnlMsgId>DD-1</OrgnlMsgId><OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
    <GrpSts>RJCT</GrpSts>
    <StsRsnInf><Rsn><Cd>DUPL</Cd></Rsn>
      <AddtlInf>MsgId already received</AddtlInf></StsRsnInf>
  </OrgnlGrpInfAndSts>
 </CstmrPmtStsRpt></Document>"#;

    let doc = parse_pain002(xml)?;
    assert!(!doc.is_fully_accepted());
    assert!(doc.rejected_transactions().is_empty()); // nothing to iterate
    assert_eq!(doc.reason_codes().len(), 1);               // but there is a reason
    assert_eq!(doc.group_additional_info, ["MsgId already received"]);
    Ok(())
}
```

`DUPL` is not one of the codes the `ReasonCode` enum names, and is carried
through as `ReasonCode::Other` rather than dropped — the reason you need must
not depend on whether the enum happens to know the code.

## Verification of Payee

DK Anlage 3 V3.9 section 2.2.6 (12 March 2025; printed page 234) defines these
VoP outcomes in pain.002.001.10. They are **DK customer-profile semantics**, not
a universal EPC mapping. The EPC103-24 inter-PSP API is separate. Legal deadlines
depend on PSP category/geography; codes do not establish liability or settlement.

[DK primary archive](https://www.ebics.de/fileadmin/unsecured/anlage3/anlage3_archiv/Anlage3_Archiv_V3_9.zip).

```rust,no_run
use sepa::{VerificationOutcome, parse_pain002};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string("status-report.xml")?;

    for block in &parse_pain002(&xml)?.payment_info_statuses {
        // A file of hundreds reports counts per outcome and itemises only
        // what needs a decision.
        for bucket in &block.status_counts {
            // `count` is an Option: the bank asserting that a status bucket
            // exists is worth reporting even when the number beside it is
            // unreadable, so the row is kept rather than dropped.
            match bucket.count {
                Some(n) => println!("{n} x {:?}", bucket.status),
                None => println!("? x {:?} (count was {:?})", bucket.status, bucket.count_raw),
            }
        }

        for tx in &block.transactions {
            match tx.status.as_ref().and_then(|s| s.verification()) {
                Some(VerificationOutcome::CloseMatch) => {
                    // Reported text is not authentication of the payee.
                    println!("close match, bank holds: {:?}", tx.additional_info);
                }
                Some(VerificationOutcome::NoMatch) => {
                    println!("no match — payer decision required under the applicable process");
                }
                Some(VerificationOutcome::NotApplicable) => {
                    println!("verification not applicable");
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
| `RCVC` | Match | Name matched; payment acceptance is separate |
| `RVMC` | Close match | Show the returned name and let the payer confirm |
| `RVNM` | No match | Escalate under the applicable payer-decision process |
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
