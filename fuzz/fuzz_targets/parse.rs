//! Parsers must never panic on bank-supplied input.
//!
//! Every parser here consumes a file that arrived over EBICS or FinTS from a
//! third party. A panic is a denial of service on the payment pipeline, so the
//! only acceptable outcome for arbitrary bytes is `Ok` or `Err`.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(xml) = std::str::from_utf8(data) else {
        return;
    };

    // Each parser independently: a panic in any of them is a bug.
    let _ = sepa::parse_pain002(xml);
    let _ = sepa::parse_camt052(xml);
    let _ = sepa::parse_camt053(xml);
    let _ = sepa::parse_camt054(xml);

    // Where a document does parse, walking the result must not panic either —
    // the accessors index into parsed data.
    if let Ok(doc) = sepa::parse_camt053(xml) {
        for stmt in &doc.statements {
            let _ = stmt.net_movement_ct();
            let _ = stmt.opening_balance();
            let _ = stmt.closing_balance();
            for entry in &stmt.entries {
                let _ = entry.signed_ct();
                let _ = entry.is_return();
                let _ = entry.end_to_end_id();
                let _ = entry.counterparty_iban();
            }
        }
    }
    if let Ok(doc) = sepa::parse_pain002(xml) {
        let _ = doc.is_fully_accepted();
        let _ = doc.has_rejections();
        let _ = doc.rejected_transactions();
    }
});
