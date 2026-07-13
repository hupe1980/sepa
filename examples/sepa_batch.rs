//! SEPA payment batch example.
//!
//! Demonstrates:
//! - IBAN and BIC validation
//! - Building a pain.008 direct-debit batch (recurring Lastschrift)
//! - Building a pain.001 credit-transfer batch (Erstattung)
//! - Integer-safe money formatting — no f64

use sepa::pain008::SequenceType;
use sepa::{
    CreditTransferEntry, DirectDebitEntry, Pain001Builder, Pain008Builder, validate_bic,
    validate_iban,
};

fn main() {
    // ── Validate IBANs and BICs ───────────────────────────────────────────────

    let creditor_iban =
        validate_iban("DE89 3704 0044 0532 0130 00").expect("creditor IBAN is valid");
    let creditor_bic = validate_bic("COBADEFFXXX").expect("creditor BIC is valid");

    let debtor_a_iban = validate_iban("NL91ABNA0417164300").expect("debtor A IBAN is valid");
    let debtor_b_iban = validate_iban("GB29NWBK60161331926819").expect("debtor B IBAN is valid");

    println!("Creditor: {} ({})", creditor_iban.as_str(), creditor_bic);
    println!("Debtor A: {}", debtor_a_iban); // Display shows grouped: "NL91 ABNA ..."
    println!("Debtor B: {}", debtor_b_iban);

    // ── pain.008 — SEPA Core Direct Debit batch ───────────────────────────────

    let pain008_xml = Pain008Builder::new("Stadtwerke Muster GmbH", &creditor_iban)
        .msg_id("DD-2026-07-001")
        .sequence_type(SequenceType::Rcur)
        .collection_date("2026-07-20")
        .creditor_bic(creditor_bic)
        .add_entry(
            DirectDebitEntry::new(
                "MND-00042",
                "2024-03-01",
                "Max Mustermann",
                debtor_a_iban,
                8_500, // 85.00 EUR — integer cents, no f64
                "ABSCHLAG-2026-07",
            )
            .with_description("Abschlag Juli 2026"),
        )
        .add_entry(
            DirectDebitEntry::new(
                "MND-00099",
                "2023-11-15",
                "Erika Mustermann",
                debtor_b_iban,
                12_300, // 123.00 EUR
                "ABSCHLAG-2026-07-B",
            )
            .with_description("Abschlag Juli 2026"),
        )
        .build_xml();

    println!("\n── pain.008 Direct Debit batch ──");
    println!("Entries:   2");
    println!("Total:     {}", sepa::ct_to_eur_str(8_500 + 12_300)); // "207.00"
    // In production, write pain008_xml to file or POST to bank API.
    assert!(pain008_xml.contains("<SeqTp>RCUR</SeqTp>"));
    assert!(pain008_xml.contains("<CtrlSum>208.00</CtrlSum>"));
    println!("XML valid: ✓");

    // ── pain.001 — SEPA Credit Transfer batch (Erstattungen) ─────────────────

    let refund_iban = validate_iban("AT611904300234573201").expect("valid");

    let pain001_xml = Pain001Builder::new("Stadtwerke Muster GmbH", &creditor_iban)
        .msg_id("CT-2026-07-001")
        .execution_date("2026-07-22")
        .add_entry(
            CreditTransferEntry::new(
                "Franz Huber",
                refund_iban,
                3_200, // 32.00 EUR Erstattung — integer cents
                "ERSTATTUNG-2025-HUBER",
            )
            .with_description("Jahresabschluss Erstattung 2025"),
        )
        .build_xml();

    println!("\n── pain.001 Credit Transfer batch ──");
    println!("Entries:   1");
    println!("Total:     {}", sepa::ct_to_eur_str(3_200)); // "32.00"
    assert!(pain001_xml.contains("<CtrlSum>32.00</CtrlSum>"));
    println!("XML valid: ✓");
}
