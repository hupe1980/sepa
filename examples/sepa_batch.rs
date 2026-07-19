//! SEPA payment batch example.
//!
//! Demonstrates:
//! - IBAN, BIC and Creditor Identifier validation
//! - A pain.008 direct debit run carrying **both** FRST and RCUR collections
//!   in one file — the reason payment groups exist
//! - A pain.001 credit transfer batch
//! - Structured ISO 11649 references and ultimate parties
//! - Integer-safe money formatting — no f64

// An example reads better with `expect` than with error plumbing on every line.
#![allow(
    clippy::expect_used,
    clippy::similar_names,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use sepa::{
    CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup, Pain001Builder,
    Pain008Builder, Party, Purpose, RfReference, SequenceType, ValidationError, validate_bic,
    validate_creditor_id, validate_iban,
};

fn main() -> Result<(), ValidationError> {
    // ── Validate identifiers ──────────────────────────────────────────────────

    let creditor_iban =
        validate_iban("DE89 3704 0044 0532 0130 00").expect("creditor IBAN is valid");
    let creditor_bic = validate_bic("COBADEFFXXX").expect("creditor BIC is valid");
    // Mandatory for SEPA Direct Debit (EPC AT-02).
    let creditor_id = validate_creditor_id("DE98ZZZ09999999999").expect("creditor ID is valid");

    let debtor_a = validate_iban("NL91ABNA0417164300").expect("debtor A IBAN is valid");
    let debtor_b = validate_iban("GB29NWBK60161331926819").expect("debtor B IBAN is valid");

    println!("Creditor: {} ({creditor_bic})", creditor_iban.as_str());
    println!("Debtor A: {debtor_a}"); // Display groups in fours
    println!("SEPA area: {}", creditor_iban.is_sepa());

    // ── pain.008 — one file, two sequence types ───────────────────────────────
    //
    // A real collection run mixes first and recurring collections. Each needs
    // its own PmtInf block, because SeqTp lives at that level.

    let pain008_xml = Pain008Builder::new("Stadtwerke Muster GmbH")
        .msg_id("DD-2026-07-001")
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke Muster GmbH",
                &creditor_iban,
                creditor_id.clone(),
            )
            .sequence_type(SequenceType::Frst)
            .collection_date("2026-07-20")
            .creditor_bic(creditor_bic.clone())
            .add_entry(
                DirectDebitEntry::new(
                    "MND-00042",
                    "2026-06-01",
                    "Max Mustermann",
                    debtor_a,
                    8_500, // 85.00 EUR — integer cents, no f64
                    "ABSCHLAG-2026-07-A",
                )
                .with_description("Abschlag Juli 2026"),
            ),
        )
        .add_group(
            DirectDebitGroup::new("Stadtwerke Muster GmbH", &creditor_iban, creditor_id)
                .sequence_type(SequenceType::Rcur)
                .collection_date("2026-07-18")
                .creditor_bic(creditor_bic)
                .add_entry(
                    DirectDebitEntry::new(
                        "MND-00099",
                        "2023-11-15",
                        "Erika Mustermann",
                        debtor_b,
                        12_300, // 123.00 EUR
                        "ABSCHLAG-2026-07-B",
                    )
                    // Collecting on behalf of the network operator.
                    .with_ultimate_creditor(Party::new("Netzbetreiber AG"))
                    .with_purpose(Purpose::Elec)
                    .with_description("Abschlag Juli 2026"),
                ),
        )
        .build()?;

    println!("\n── pain.008 Direct Debit run ──");
    println!("Groups:  2 (FRST + RCUR in one file)");
    println!("Total:   {}", sepa::ct_to_eur_str(8_500 + 12_300));
    assert!(pain008_xml.contains("<SeqTp>FRST</SeqTp>"));
    assert!(pain008_xml.contains("<SeqTp>RCUR</SeqTp>"));
    assert!(pain008_xml.contains("<CtrlSum>208.00</CtrlSum>"));
    println!("XML valid: ok");

    // ── pain.001 — credit transfer with a structured reference ────────────────

    let refund_iban = validate_iban("AT611904300234573201").expect("valid");
    // A self-checking invoice reference that survives the round trip to camt.
    let reference = RfReference::generate("ERSTATTUNG-2025-HUBER").expect("valid reference");
    println!("\nRF reference: {reference}"); // grouped for printing

    let pain001_xml = Pain001Builder::new("Stadtwerke Muster GmbH")
        .msg_id("CT-2026-07-001")
        .add_group(
            CreditTransferGroup::new("Stadtwerke Muster GmbH", &creditor_iban)
                .execution_date("2026-07-22")
                .add_entry(
                    CreditTransferEntry::new(
                        "Franz Huber",
                        refund_iban,
                        3_200, // 32.00 EUR Erstattung
                        "ERSTATTUNG-2025",
                    )
                    .with_reference(reference),
                ),
        )
        .build()?;

    println!("\n── pain.001 Credit Transfer batch ──");
    println!("Total:   {}", sepa::ct_to_eur_str(3_200));
    assert!(pain001_xml.contains("<CtrlSum>32.00</CtrlSum>"));
    // pain.001.001.09 wraps the execution date in a <Dt> choice child.
    assert!(pain001_xml.contains("<ReqdExctnDt><Dt>2026-07-22</Dt></ReqdExctnDt>"));
    assert!(pain001_xml.contains("<Cd>SCOR</Cd>"));
    println!("XML valid: ok");

    // ── Validation and transliteration ────────────────────────────────────────

    let umlaut_xml = Pain001Builder::new("Müller & Söhne GmbH")
        .msg_id("CT-UMLAUT")
        .add_group(
            CreditTransferGroup::new("Müller & Söhne GmbH", &creditor_iban)
                .execution_date("2026-07-22")
                .add_entry(CreditTransferEntry::new(
                    "Jörg Groß",
                    creditor_iban.clone(),
                    1_000,
                    "E2E-UMLAUT",
                )),
        )
        .build()?;
    assert!(umlaut_xml.contains("Mueller + Soehne GmbH"));
    assert!(umlaut_xml.contains("Joerg Gross"));

    // Invalid batches are rejected instead of producing a file the bank refuses.
    let rejected = Pain001Builder::new("Acme GmbH")
        .msg_id("CT-BAD")
        .build()
        .expect_err("an empty batch must be rejected");

    println!("\n── Validation ──");
    println!("Transliterated: Müller & Söhne GmbH -> Mueller + Soehne GmbH");
    println!("Empty batch:    {rejected}");

    Ok(())
}
