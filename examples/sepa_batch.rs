//! SEPA payment batch example.
//!
//! Demonstrates:
//! - IBAN, BIC and Creditor Identifier validation
//! - A pain.008 direct debit run carrying **both** FRST and RCUR collections
//!   in one file — the reason payment groups exist
//! - A pain.001 credit transfer batch
//! - A camt.055 recall of one collection, *before* it settles
//! - A pain.007 reversal of one of those collections, *after* it settles
//! - Structured ISO 11649 references and ultimate parties
//! - Structured postal addresses, ready for the 15 Nov 2026 EPC cut-over
//! - Typed `IsoDate` values, so no date is ever hand-formatted
//! - Integer-safe money formatting — no f64
//! - Build errors that name the group and transaction that failed

// An example reads better with `expect` than with error plumbing on every line.
#![allow(
    clippy::expect_used,
    clippy::similar_names,
    clippy::unwrap_used,
    clippy::too_many_lines
)]

use sepa::{
    Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason, CreditTransferEntry,
    CreditTransferGroup, DirectDebitEntry, DirectDebitGroup, IsoDate, OriginalMessage,
    Pain001Builder, Pain007Builder, Pain008Builder, Party, PostalAddress, Purpose, RejectionReason,
    ReversalEntry, ReversalGroup, ReversalReason, RfReference, SequenceType, parse_camt029,
    validate_bic, validate_creditor_id, validate_iban,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Validate identifiers ──────────────────────────────────────────────────

    let creditor_iban =
        validate_iban("DE89 3704 0044 0532 0130 00").expect("creditor IBAN is valid");
    let creditor_bic = validate_bic("COBADEFFXXX").expect("creditor BIC is valid");
    // Mandatory for SEPA Direct Debit (EPC AT-02).
    let creditor_id = validate_creditor_id("DE98ZZZ09999999999").expect("creditor ID is valid");

    let debtor_a = validate_iban("NL91ABNA0417164300").expect("debtor A IBAN is valid");
    let debtor_b = validate_iban("GB29NWBK60161331926819").expect("debtor B IBAN is valid");

    // From 15 November 2026 an address the EPC schemes accept must carry a town
    // and a country, so `PostalAddress` takes both up front — the unstructured
    // form is simply not constructible.
    let creditor_address = PostalAddress::new("Musterstadt", "DE")?
        .street("Rathausplatz")
        .building_number("1")
        .post_code("12345");

    println!("Creditor: {} ({creditor_bic})", creditor_iban.as_str());
    println!("Debtor A: {debtor_a}"); // Display groups in fours
    println!("SEPA area: {}", creditor_iban.is_sepa());

    // ── pain.008 — one file, two sequence types ───────────────────────────────
    //
    // A real collection run mixes first and recurring collections. Each needs
    // its own PmtInf block, because SeqTp lives at that level.

    // Held in variables so the reversal below can be built from them rather
    // than from hand-retyped values.
    let first_group = DirectDebitGroup::new(
        "Stadtwerke Muster GmbH",
        &creditor_iban,
        &creditor_id,
        IsoDate::new(2026, 7, 20)?,
    )
    .sequence_type(SequenceType::Frst)
    .creditor_bic(creditor_bic.clone())
    .creditor_address(creditor_address.clone());
    let first_entry = DirectDebitEntry::new(
        "MND-00042",
        "2026-06-01".parse()?, // rejected here if malformed, not by the bank
        "Max Mustermann",
        debtor_a,
        8_500, // 85.00 EUR — integer cents, no f64
        "ABSCHLAG-2026-07-A",
    )
    .with_description("Abschlag Juli 2026");

    let pain008 = Pain008Builder::new("Stadtwerke Muster GmbH", "DD-2026-07-001")
        // Pinned, so the file regenerates byte-for-byte — and so a camt.055
        // recall can quote the `OrgnlCreDtTm` the bank actually received.
        .created_at("2026-07-15T09:00:00".parse()?)
        .add_group(first_group.clone().add_entry(first_entry.clone()))
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke Muster GmbH",
                &creditor_iban,
                &creditor_id,
                IsoDate::new(2026, 7, 18)?,
            )
            .sequence_type(SequenceType::Rcur)
            .creditor_bic(creditor_bic)
            .add_entry(
                DirectDebitEntry::new(
                    "MND-00099",
                    "2023-11-15".parse()?,
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
        );
    let pain008_xml = pain008.build()?;

    println!("\n── pain.008 Direct Debit run ──");
    println!("Groups:  2 (FRST + RCUR in one file)");
    println!("Total:   {}", sepa::ct_to_eur_str(8_500 + 12_300));
    assert!(pain008_xml.contains("<SeqTp>FRST</SeqTp>"));
    assert!(pain008_xml.contains("<SeqTp>RCUR</SeqTp>"));
    assert!(pain008_xml.contains("<CtrlSum>208.00</CtrlSum>"));
    assert!(pain008_xml.contains("<TwnNm>Musterstadt</TwnNm><Ctry>DE</Ctry>"));
    println!("XML valid: ok");

    // ── camt.055 — recall one collection before it settles ────────────────────
    //
    // The cheap correction. A recall is a *request*: the bank may refuse it, and
    // nothing is cancelled until the camt.029 answer says so. Once the
    // collection settles this route closes and only pain.007 is left.

    let recall_xml = Camt055Builder::new(
        "CXL-2026-07-001",
        "Stadtwerke Muster GmbH",     // Assgnr — you
        validate_bic("COBADEFFXXX")?, // Assgne — your bank
        // Copied from the builder that produced the file, not retyped: MsgId,
        // the pinned CreDtTm and the totals all come across.
        OriginalMessage::from_direct_debit(&pain008),
    )
    .case_id("CASE-2026-07-001")
    .add_group(
        CancellationGroup::new("DD-2026-07-001").add_entry(
            CancellationEntry::new("ABSCHLAG-2026-07-A", CancellationReason::Dupl)
                .original_amount(8_500)
                .additional_info("Doppelte Einreichung"),
        ),
    )
    .build()?;

    println!("\n── camt.055 Recall ──");
    println!("Recalling: {}", sepa::ct_to_eur_str(8_500));
    assert!(recall_xml.contains("<CstmrPmtCxlReq>"));
    assert!(recall_xml.contains("<OrgnlEndToEndId>ABSCHLAG-2026-07-A</OrgnlEndToEndId>"));
    assert!(recall_xml.contains("<Cd>DUPL</Cd>"));
    println!("XML valid: ok");

    // ── camt.029 — and the answer, which may be "too late" ────────────────────
    //
    // `PDCR` is neither outcome: the case is open. Only `ARDT` — already
    // settled — says the recall window has closed and a reversal is what is
    // left. That is the branch the next section takes.

    let answer = parse_camt029(
        r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-1</Id><CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-2026-07-001</Id></RslvdCase>
    <Sts><Conf>RJCR</Conf></Sts>
    <CxlDtls><OrgnlGrpInfAndSts>
      <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
      <GrpCxlSts>RJCR</GrpCxlSts>
      <CxlStsRsnInf><Rsn><Cd>ARDT</Cd></Rsn></CxlStsRsnInf>
    </OrgnlGrpInfAndSts></CxlDtls>
  </RsltnOfInvstgtn></Document>"#,
    )?;

    println!("\n── camt.029 Answer ──");
    println!("Case:     {:?}", answer.resolved_case_id);
    println!("Final:    {}", answer.is_final());
    println!("Accepted: {}", answer.is_accepted());
    // The refusal sits at group level with no transaction blocks at all — a
    // reader that walks only TxInfAndSts sees an empty document.
    assert_eq!(answer.transactions().count(), 0);
    assert_eq!(answer.rejection_reasons(), [&RejectionReason::Ardt]);
    let too_late = answer.rejection_reasons().iter().any(|r| r.is_too_late());
    println!("Too late: {too_late} — a pain.007 reversal is the remaining route");

    // ── pain.007 — reverse one of those collections ───────────────────────────
    //
    // The collection settled, then turned out to be wrong. `reverse` copies the
    // mandate, creditor identifier, scheme, sequence type, collection date and
    // both parties from the objects that produced it, so the reversal cannot
    // disagree with what was actually sent.

    let pain007_xml = Pain007Builder::new(
        "Stadtwerke Muster GmbH",
        "DD-2026-07-001",
        "STORNO-2026-07-001",
    )
    .creditor_agent(validate_bic("COBADEFFXXX")?)
    .add_group(
        ReversalGroup::new("DD-2026-07-001").add_entry(ReversalEntry::reverse(
            &first_group,
            &first_entry,
            ReversalReason::Ms02,
        )),
    )
    .build()?;

    println!("\n── pain.007 Reversal ──");
    println!("Reversing: {}", sepa::ct_to_eur_str(8_500));
    assert!(pain007_xml.contains("<CstmrPmtRvsl>"));
    assert!(pain007_xml.contains("<OrgnlEndToEndId>ABSCHLAG-2026-07-A</OrgnlEndToEndId>"));
    assert!(pain007_xml.contains("<RvsdInstdAmt Ccy=\"EUR\">85.00</RvsdInstdAmt>"));
    assert!(pain007_xml.contains("<MndtId>MND-00042</MndtId>"));
    println!("XML valid: ok");

    // ── pain.001 — credit transfer with a structured reference ────────────────

    let refund_iban = validate_iban("AT611904300234573201").expect("valid");
    // A self-checking invoice reference that survives the round trip to camt.
    let reference = RfReference::generate("ERSTATTUNG-2025-HUBER").expect("valid reference");
    println!("\nRF reference: {reference}"); // grouped for printing

    let pain001_xml = Pain001Builder::new("Stadtwerke Muster GmbH", "CT-2026-07-001")
        .add_group(
            CreditTransferGroup::new(
                "Stadtwerke Muster GmbH",
                &creditor_iban,
                IsoDate::new(2026, 7, 22)?,
            )
            .debtor_address(creditor_address)
            .add_entry(
                CreditTransferEntry::new(
                    "Franz Huber",
                    refund_iban,
                    3_200, // 32.00 EUR Erstattung
                    "ERSTATTUNG-2025",
                )
                .with_reference(reference)
                // Hybrid: the town and country are structured, the rest is
                // one free-text line.
                .with_creditor_address(PostalAddress::new("Wien", "AT")?.line("Stephansplatz 3/2")),
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

    let umlaut_xml = Pain001Builder::new("Müller & Söhne GmbH", "CT-UMLAUT")
        .add_group(
            CreditTransferGroup::new(
                "Müller & Söhne GmbH",
                &creditor_iban,
                IsoDate::new(2026, 7, 22)?,
            )
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
    let rejected = Pain001Builder::new("Acme GmbH", "CT-BAD")
        .build()
        .expect_err("an empty batch must be rejected");

    // A field-level failure names the group and transaction it came from, so a
    // rejected run points at the row to fix rather than at the whole file.
    let located = Pain008Builder::new("Stadtwerke Muster GmbH", "DD-BAD")
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke Muster GmbH",
                &creditor_iban,
                &creditor_id,
                IsoDate::new(2026, 7, 20)?,
            )
            .add_entry(DirectDebitEntry::new(
                "MND-1",
                "2024-01-01".parse()?,
                "Erste Kundin",
                creditor_iban.clone(),
                100,
                "E2E-1",
            ))
            .add_entry(DirectDebitEntry::new(
                "MND-2",
                "2024-01-01".parse()?,
                "Zweiter Kunde",
                creditor_iban.clone(),
                0, // a zero amount is outside the SEPA range
                "E2E-2",
            )),
        )
        .build()
        .expect_err("a zero amount must be rejected");

    println!("\n── Validation ──");
    println!("Transliterated: Müller & Söhne GmbH -> Mueller + Soehne GmbH");
    println!("Empty batch:    {rejected}");
    println!("Located:        {located}");
    println!("  group:        {:?}", located.location.group);
    println!("  transaction:  {:?}", located.location.transaction);

    // A malformed date cannot reach a batch at all.
    let bad_date = "2026-02-30".parse::<IsoDate>().expect_err("30 February");
    println!("Bad date:       {bad_date}");

    Ok(())
}
