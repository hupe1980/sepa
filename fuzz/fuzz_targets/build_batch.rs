//! Builders must never panic, whatever text and amounts they are handed.
//!
//! `build` may reject the batch — that is the point — but it must decide,
//! not crash. Any batch it *accepts* must also be well-formed XML whose text
//! is inside the SEPA character set.
#![no_main]

use libfuzzer_sys::fuzz_target;
use sepa::pain001::CreditTransferSchema;
use sepa::pain008::DirectDebitSchema;

fuzz_target!(|data: (&str, &str, i64, u8)| {
    let (text, id, amount, selector) = data;
    let Ok(iban) = sepa::validate_iban("DE89370400440532013000") else {
        return;
    };
    // Every schema version must survive the same inputs; the older ones name
    // elements differently and are the least exercised by the unit tests.
    let pick = usize::from(selector) % 3;
    let date = sepa::IsoDate::today();

    // A structured address built from the same arbitrary text: the builders
    // must reject or emit, never crash, and the DK schemas must refuse it.
    let address = sepa::PostalAddress::new(if text.is_empty() { "Berlin" } else { text }, "DE")
        .unwrap_or_else(|_| {
            sepa::PostalAddress::new("Berlin", "DE").expect("a literal address is valid")
        })
        .street(text)
        .line(text);

    let sct = sepa::Pain001Builder::new(text, id)
        .schema(CreditTransferSchema::ALL[pick])
        .add_group(
            sepa::CreditTransferGroup::new(text, &iban, date)
                .payment_info_id(id)
                .add_entry(
                    sepa::CreditTransferEntry::new(text, iban.clone(), amount, id)
                        // The proprietary branch is the one whose `Issr` is
                        // caller text all the way to the wire.
                        .with_remittance(sepa::RemittanceInfo::Proprietary {
                            reference: id.to_owned(),
                            issuer: Some(text.to_owned()),
                        })
                        .with_ultimate_debtor(sepa::Party::new(text))
                        .with_creditor_address(address.clone()),
                ),
        );
    let _ = sct.total_ct();
    if let Ok(xml) = sct.build() {
        assert!(xml.starts_with("<?xml"), "accepted batch must be a document");
        assert!(xml.ends_with("</Document>"));
        assert_sepa_text(&xml);
    }

    if let Ok(ci) = sepa::validate_creditor_id("DE98ZZZ09999999999") {
        let sdd = sepa::Pain008Builder::new(text, id)
            .schema(DirectDebitSchema::ALL[pick])
            .add_group(
                sepa::DirectDebitGroup::new(text, &iban, &ci, date)
                    .payment_info_id(id)
                    .creditor_address(address.clone())
                    .add_entry(
                        sepa::DirectDebitEntry::new(id, date, text, iban.clone(), amount, id)
                            .with_description(text)
                            .with_amendment(sepa::MandateAmendment::debtor_account_changed()),
                    ),
            );
        let _ = sdd.total_ct();
        if let Ok(xml) = sdd.build() {
            assert!(xml.starts_with("<?xml"));
            assert!(xml.ends_with("</Document>"));
            assert_sepa_text(&xml);
        }

        // A reversal of that same collection: `reverse` copies arbitrary text
        // out of the collection objects, so it sees everything the builders do.
        let group = sepa::DirectDebitGroup::new(text, &iban, &ci, date);
        let entry = sepa::DirectDebitEntry::new(id, date, text, iban.clone(), amount, id);
        let rvsl = sepa::Pain007Builder::new(text, id, id)
            .add_group(sepa::ReversalGroup::new(id).add_entry(sepa::ReversalEntry::reverse(
                &group,
                &entry,
                sepa::ReversalReason::Ms02,
            )));
        let _ = rvsl.total_ct();
        if let Ok(xml) = rvsl.build() {
            assert!(xml.starts_with("<?xml"));
            assert!(xml.ends_with("</Document>"));
            assert_sepa_text(&xml);
            // A reversal may never send back more than was collected.
            assert!(rvsl.total_ct() <= sdd.total_ct());
        }

        // A recall of that same submission. `CancellationReason` carries
        // arbitrary text into `Prtry`, and `AddtlInf` is the only Max105Text
        // element in the crate — both reach the wire.
        let Ok(bank) = sepa::validate_bic("COBADEFFXXX") else {
            return;
        };
        let recall = sepa::Camt055Builder::new(
            id,
            text,
            bank,
            sepa::OriginalMessage::from_direct_debit(&sdd),
        )
        .case_id(id)
        .add_group(
            sepa::CancellationGroup::new(id).add_entry(
                sepa::CancellationEntry::new(id, text.parse().unwrap_or(sepa::CancellationReason::Cust))
                    .original_amount(amount)
                    .additional_info(text),
            ),
        );
        let _ = recall.total_ct();
        if let Ok(xml) = recall.build() {
            assert!(xml.starts_with("<?xml"));
            assert!(xml.ends_with("</Document>"));
            assert_sepa_text(&xml);
        }
    }
});

/// Every text node of an accepted document must be inside the SEPA character
/// set — the invariant `RmtInf/Strd/CdtrRefInf/Tp/Issr` silently broke for
/// three releases, because it was asserted per named tag rather than per node.
fn assert_sepa_text(xml: &str) {
    for chunk in xml.split('>').skip(1) {
        let Some(text) = chunk.split('<').next() else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let raw = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'");
        assert!(
            sepa::is_sepa_text(&raw),
            "emitted text {raw:?} is not in the SEPA character set"
        );
    }
}
