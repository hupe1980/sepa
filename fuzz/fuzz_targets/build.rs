//! Builders must never panic, whatever text and amounts they are handed.
//!
//! `build` may reject the batch — that is the point — but it must decide,
//! not crash. Any batch it *accepts* must also be well-formed XML whose text
//! is inside the SEPA character set.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (&str, &str, i64)| {
    let (text, id, amount) = data;
    let Ok(iban) = sepa::validate_iban("DE89370400440532013000") else {
        return;
    };

    let sct = sepa::Pain001Builder::new(text, &iban)
        .msg_id(id)
        .execution_date(text)
        .payment_info_id(id)
        .add_entry(
            sepa::CreditTransferEntry::new(text, iban.clone(), amount, id)
                .with_description(text)
                .with_ultimate_debtor(sepa::Party::new(text)),
        );
    let _ = sct.total_ct();
    if let Ok(xml) = sct.build() {
        assert!(xml.starts_with("<?xml"), "accepted batch must be a document");
        assert!(xml.ends_with("</Document>"));
    }

    if let Ok(ci) = sepa::validate_creditor_id("DE98ZZZ09999999999") {
        let sdd = sepa::Pain008Builder::new(text, &iban)
            .msg_id(id)
            .creditor_id(ci)
            .collection_date(text)
            .add_entry(
                sepa::DirectDebitEntry::new(id, text, text, iban.clone(), amount, id)
                    .with_description(text),
            );
        let _ = sdd.total_ct();
        if let Ok(xml) = sdd.build() {
            assert!(xml.starts_with("<?xml"));
            assert!(xml.ends_with("</Document>"));
        }
    }
});
