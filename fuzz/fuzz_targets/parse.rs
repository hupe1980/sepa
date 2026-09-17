//! Parsers must never panic on bank-supplied input — **and must never invent a
//! number**.
//!
//! Every parser here consumes a file that arrived over EBICS or FinTS from a
//! third party. A panic is a denial of service on the payment pipeline, so the
//! only acceptable outcome for arbitrary bytes is `Ok` or `Err`.
//!
//! "Does not panic" used to be this target's *entire* invariant: every accessor
//! was called and its result thrown away with `let _ =`. That is why the
//! fuzzer ran for three releases over a parser that read an unrecognised
//! `CdtDbtInd` as a credit — **a fabricated value is a perfectly ordinary
//! `Ok`**, and nothing here was looking at the values. The write-path target
//! (`build_batch`) had asserted real invariants all along; the read path had
//! none.
//!
//! So the money invariants are asserted here now, on every document that
//! parses, whatever bytes produced it. See `tests/conformance.rs` for the
//! same properties over hand-built near-misses.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };

    // Each parser independently: a panic in any of them is a bug.
    if let Ok(doc) = sepa::parse_pain002(xml) {
        let _ = doc.is_fully_accepted();
        let _ = doc.has_rejections();
        for tx in doc.rejected_transactions() {
            let _ = tx.is_rejected();
        }
        for count in &doc.group_status_counts {
            let _ = count.status.verification();
        }
        for block in &doc.payment_info_statuses {
            let _ = block.has_rejections();
            let _ = block.rejection_reasons();
            for count in &block.status_counts {
                let _ = (count.count, count.total_ct);
            }
            for tx in &block.transactions {
                // Verification of Payee: the outcome must be derivable from any
                // status a bank sends, including ones this crate does not know.
                let _ = tx.status.as_ref().map(|s| (s.verification(), s.is_verification()));
                let _ = tx.additional_info.len();
            }
        }
    }
    // camt.029 answers a recall, so every accessor a consumer reaches for
    // before deciding whether money is coming back must survive junk too.
    if let Ok(doc) = sepa::parse_camt029(xml) {
        let _ = doc.is_accepted();
        let _ = doc.has_rejections();
        let _ = doc.is_final();
        let _ = doc.rejection_reasons();
        for tx in doc.transactions() {
            let _ = (tx.is_accepted(), tx.is_rejected());
            let _ = tx.original_execution_date();
            let _ = tx.original_collection_date();
        }
        for group in &doc.groups {
            let _ = group.all_transactions().count();
            for p in &group.payment_infos {
                let _ = p.has_rejections();
            }
        }
    }
    let _ = sepa::parse_camt052(xml);
    let _ = sepa::parse_camt053(xml);
    let _ = sepa::parse_camt054(xml);

    // Where a document does parse, walking the result must not panic — and
    // every figure it reports must follow from what arrived.
    if let Ok(doc) = sepa::parse_camt053(xml) {
        for stmt in &doc.statements {
            for balance in &stmt.balances {
                assert_money(balance.signed_ct(), balance.amount.ct, balance.amount.direction);
                assert_indicator(balance.amount.direction, balance.amount.direction_raw.as_deref());
            }
            for entry in &stmt.entries {
                assert_money(entry.signed_ct(), entry.amount.ct, entry.amount.direction);
                assert_indicator(entry.amount.direction, entry.amount.direction_raw.as_deref());
                let _ = entry.is_return();
                let _ = entry.end_to_end_id();
                let _ = entry.counterparty_iban();

                for detail in &entry.details {
                    // A detail is the one level with a *weaker* invariant, and
                    // deliberately so: a sole detail with no itemised amount
                    // inherits the entry's, which is safe precisely because
                    // there is only one transaction to attribute it to. So its
                    // figure may outlive its own magnitude — but never its
                    // direction, and never the entry's magnitude.
                    if detail.signed_ct().is_some() {
                        assert!(
                            detail.amount.direction.is_some(),
                            "a detail figure still requires a direction"
                        );
                        assert!(
                            detail.amount.ct.is_some() || entry.amount.ct.is_some(),
                            "a detail figure must come from its own amount or its entry's"
                        );
                    }
                    if let (Some(signed), Some(own)) = (detail.signed_ct(), detail.amount.ct) {
                        assert_eq!(
                            signed.unsigned_abs(),
                            own.unsigned_abs(),
                            "signing must not change a detail's magnitude"
                        );
                    }
                }
                if let Some(charges) = &entry.charges {
                    for record in &charges.records {
                        assert_money(record.signed_ct(), record.amount.ct, record.amount.direction);
                    }
                    // All-or-nothing: a total exists only if every record did.
                    assert_eq!(
                        charges.total_signed_ct().is_some(),
                        charges.records.iter().all(|r| r.signed_ct().is_some()),
                        "a charge total must not sum past a record it could not resolve"
                    );
                }
                // Detail amounts are resolved from three different places and
                // summed, so overflow must saturate into `None`, not panic.
                assert_eq!(
                    entry.details_signed_sum_ct().is_some(),
                    entry.details.iter().all(|d| d.signed_ct().is_some()),
                    "a detail sum must not skip a detail it could not resolve"
                );
                let _ = entry.details_reconcile();
            }
            // Likewise one level up: a net movement that quietly omits the rows
            // it could not read is a number that looks right and is not.
            assert_eq!(
                stmt.net_movement_ct().is_some(),
                stmt.entries.iter().all(|e| e.signed_ct().is_some()),
                "a net movement must not skip an entry it could not resolve"
            );
            let _ = stmt.opening_balance();
            let _ = stmt.closing_balance();
        }
    }
    if let Ok(doc) = sepa::parse_pain002(xml) {
        let _ = doc.is_fully_accepted();
        let _ = doc.has_rejections();
        let _ = doc.rejected_transactions();
    }
});


/// A ledger figure exists exactly when both of its parts do, and signing changes
/// the sign and nothing else.
///
/// Money has a magnitude and a direction. Losing either means there is no
/// figure — not a plausible one.
fn assert_money(
    signed: Option<i64>,
    magnitude: Option<i64>,
    indicator: Option<sepa::CreditDebitIndicator>,
) {
    assert_eq!(
        signed.is_some(),
        magnitude.is_some() && indicator.is_some(),
        "a ledger figure requires a magnitude AND a direction"
    );
    if let (Some(signed), Some(magnitude), Some(indicator)) = (signed, magnitude, indicator) {
        assert_eq!(
            signed.unsigned_abs(),
            magnitude.unsigned_abs(),
            "signing must not change the magnitude"
        );
        assert_eq!(
            signed < 0,
            indicator == sepa::CreditDebitIndicator::Debit && magnitude != 0,
            "the sign must follow the indicator"
        );
    }
}

/// A direction is reported only when the file actually stated one this crate
/// recognises.
///
/// This is the assertion that would have caught the sign-flip defect: no
/// sequence of bytes may produce a resolved direction whose own raw text does
/// not parse back to it.
fn assert_indicator(indicator: Option<sepa::CreditDebitIndicator>, raw: Option<&str>) {
    if let Some(indicator) = indicator {
        let raw = raw.expect("a resolved direction must have come from somewhere");
        assert_eq!(
            raw.trim().to_ascii_uppercase(),
            indicator.as_code(),
            "a direction must round-trip to the text it was read from"
        );
    }
}
