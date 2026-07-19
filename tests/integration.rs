//! End-to-end tests: schema validation of generated files, and round-trips
//! between the builders and the parsers.
//!
//! ## XSD validation
//!
//! The tests in [`xsd`] shell out to `xmllint` and validate generated documents
//! against the real ISO 20022 schemas pinned in `tests/xsd/`. They are skipped
//! with a printed notice when `xmllint` is unavailable, so the suite still runs
//! on a bare machine — CI installs `libxml2-utils` to make sure they execute.
//!
//! Schema validation is necessary but **not sufficient**: the ISO schemas
//! permit plenty that banks reject (a zero amount, five decimal places,
//! `<BICFI>NOTPROVIDED</BICFI>`). Those rules are covered by the EPC-level
//! tests below and by the unit tests in `src/validate.rs`.

// In tests, `unwrap()` and indexing are the assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::process::Command;

use sepa::pain001::CreditTransferSchema;
use sepa::pain008::DirectDebitSchema;
use sepa::{
    CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup, Pain001Builder,
    Pain008Builder, ValidationError, parse_camt053, parse_pain002, validate_creditor_id,
    validate_iban,
};

// ── fixtures ──────────────────────────────────────────────────────────────────

fn debtor() -> sepa::Iban {
    validate_iban("DE89370400440532013000").unwrap()
}

fn creditor() -> sepa::Iban {
    validate_iban("NL91ABNA0417164300").unwrap()
}

fn creditor_id() -> sepa::CreditorId {
    validate_creditor_id("DE98ZZZ09999999999").unwrap()
}

fn sct(schema: CreditTransferSchema) -> String {
    Pain001Builder::new("Acme GmbH")
        .schema(schema)
        .msg_id("CT-2026-07-001-MAXLEN-PADDING-XXXXX")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor())
                .execution_date("2026-07-20")
                .debtor_bic("COBADEFF".parse().unwrap())
                .add_entry(
                    CreditTransferEntry::new("Supplier AG", creditor(), 12_000, "INV-2026-001")
                        .with_bic("ABNANL2A".parse().unwrap())
                        .with_description("Rechnung 2026-07-001"),
                )
                .add_entry(CreditTransferEntry::new(
                    "Second Payee",
                    creditor(),
                    3_450,
                    "INV-2026-002",
                )),
        )
        .build()
        .expect("batch is valid")
}

fn sdd(schema: DirectDebitSchema) -> String {
    Pain008Builder::new("Stadtwerke GmbH")
        .schema(schema)
        .msg_id("DD-2026-07-001")
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &debtor(), creditor_id())
                .collection_date("2026-07-20")
                .creditor_bic("COBADEFF".parse().unwrap())
                .add_entry(
                    DirectDebitEntry::new(
                        "MND-00042",
                        "2024-06-01",
                        "Max Mustermann",
                        creditor(),
                        7_500,
                        "R2026-07-001",
                    )
                    .with_description("Abschlag Juli 2026"),
                ),
        )
        .build()
        .expect("batch is valid")
}

// ── XSD validation ────────────────────────────────────────────────────────────

mod xsd {
    use super::{Command, CreditTransferSchema, DirectDebitSchema, sct, sdd};

    /// Validate `xml` against `schema_file` in `tests/xsd/`.
    ///
    /// Returns `None` when `xmllint` is not installed.
    /// Tests run in parallel and several share a schema, so the document path
    /// must be unique per call or they clobber each other's input.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn xmllint(xml: &str, schema_file: &str) -> Option<Result<(), String>> {
        let schema = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/xsd/").to_owned() + schema_file;
        if !std::path::Path::new(&schema).exists() {
            return None;
        }

        let dir = std::env::temp_dir().join(format!("sepa-xsd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let doc = dir.join(format!("{schema_file}.{n}.xml"));
        std::fs::write(&doc, xml).ok()?;

        let out = Command::new("xmllint")
            .args(["--noout", "--schema", &schema])
            .arg(&doc)
            .output()
            .ok()?;

        let result = if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        };
        std::fs::remove_file(&doc).ok();
        Some(result)
    }

    /// Assert `xml` validates, or skip loudly if `xmllint` is missing.
    fn assert_validates(xml: &str, schema_file: &str) {
        match xmllint(xml, schema_file) {
            Some(Ok(())) => {}
            Some(Err(err)) => {
                panic!("{schema_file} validation failed:\n{err}\n\n--- document ---\n{xml}")
            }
            None => eprintln!("SKIP: xmllint or {schema_file} unavailable"),
        }
    }

    #[test]
    fn pain001_001_09_validates() {
        assert_validates(&sct(CreditTransferSchema::IsoV9), "pain.001.001.09.xsd");
    }

    #[test]
    fn pain001_003_03_validates() {
        assert_validates(&sct(CreditTransferSchema::DkV2_7), "pain.001.003.03.xsd");
    }

    #[test]
    fn pain008_003_02_validates() {
        assert_validates(&sdd(DirectDebitSchema::DkV2_7), "pain.008.003.02.xsd");
    }

    #[test]
    fn pain008_001_08_validates() {
        assert_validates(&sdd(DirectDebitSchema::IsoV8), "pain.008.001.08.xsd");
    }

    #[test]
    fn structured_rf_remittance_validates() {
        // The ISO 11649 path: RmtInf/Strd/CdtrRefInf with Cd=SCOR and Issr=ISO.
        let rf = sepa::RfReference::generate("539007547034").unwrap();
        assert_eq!(rf.as_str(), "RF18539007547034");

        let xml = super::Pain001Builder::new("Acme GmbH")
            .msg_id("CT-RF-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor())
                    .execution_date("2026-07-20")
                    .add_entry(
                        super::CreditTransferEntry::new(
                            "Supplier AG",
                            super::creditor(),
                            12_000,
                            "E2E-1",
                        )
                        .with_reference(rf),
                    ),
            )
            .build()
            .unwrap();

        assert!(xml.contains("<CdOrPrtry><Cd>SCOR</Cd></CdOrPrtry>"));
        assert!(xml.contains("<Issr>ISO</Issr>"));
        assert!(xml.contains("<Ref>RF18539007547034</Ref>"));
        assert_validates(&xml, "pain.001.001.09.xsd");

        // The EPC caps the whole <Strd> block at 140 characters including tags,
        // so it must be emitted minified even though the rest is indented.
        let strd = xml
            .split("<Strd>")
            .nth(1)
            .unwrap()
            .split("</Strd>")
            .next()
            .unwrap();
        assert!(
            !strd.contains('\n'),
            "Strd must be minified to stay inside the 140-character budget"
        );
        assert!(strd.chars().count() <= 140);
    }

    #[test]
    fn structured_remittance_validates_for_direct_debit() {
        let rf = sepa::RfReference::generate("INV20260042").unwrap();
        let xml = super::Pain008Builder::new("Stadtwerke GmbH")
            .msg_id("DD-RF-001")
            .add_group(
                super::DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &super::debtor(),
                    super::creditor_id(),
                )
                .collection_date("2026-07-20")
                .add_entry(
                    super::DirectDebitEntry::new(
                        "MND-1",
                        "2024-06-01",
                        "Max Mustermann",
                        super::creditor(),
                        7_500,
                        "E2E-1",
                    )
                    .with_reference(rf),
                ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<Cd>SCOR</Cd>"));
        assert_validates(&xml, "pain.008.001.08.xsd");
    }

    #[test]
    fn ultimate_parties_and_purpose_validate_in_sequence() {
        // Element order is fixed by xs:sequence, so a misplaced UltmtDbtr or
        // Purp fails the XSD even though the elements themselves are legal.
        let xml = super::Pain001Builder::new("Acme GmbH")
            .msg_id("CT-ULT-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor())
                    .execution_date("2026-07-20")
                    .add_entry(
                        super::CreditTransferEntry::new(
                            "Supplier AG",
                            super::creditor(),
                            12_000,
                            "E2E-1",
                        )
                        .with_bic("ABNANL2A".parse().unwrap())
                        .with_ultimate_debtor(sepa::Party::new("Tochter AG"))
                        .with_ultimate_creditor(
                            sepa::Party::new("Endbeguenstigter GmbH")
                                .with_organisation_id("CUST-4711", Some("CUST")),
                        )
                        .with_purpose(sepa::Purpose::Supp)
                        .with_description("Rechnung 2026-07"),
                    ),
            )
            .build()
            .unwrap();

        assert!(xml.contains("<UltmtDbtr><Nm>Tochter AG</Nm></UltmtDbtr>"));
        assert!(xml.contains("<Id><OrgId><Othr><Id>CUST-4711</Id>"));
        assert!(xml.contains("<Purp><Cd>SUPP</Cd></Purp>"));
        assert_validates(&xml, "pain.001.001.09.xsd");
    }

    #[test]
    fn direct_debit_ultimate_parties_and_amendment_validate() {
        let xml = super::Pain008Builder::new("Stadtwerke GmbH")
            .msg_id("DD-AMD-001")
            .add_group(
                super::DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &super::debtor(),
                    super::creditor_id(),
                )
                .collection_date("2026-07-20")
                .add_entry(
                    super::DirectDebitEntry::new(
                        "MND-1",
                        "2024-06-01",
                        "Max Mustermann",
                        super::creditor(),
                        7_500,
                        "E2E-1",
                    )
                    .with_ultimate_creditor(sepa::Party::new("Netzbetreiber AG"))
                    .with_ultimate_debtor(sepa::Party::new("Erika Mustermann"))
                    .with_purpose(sepa::Purpose::Elec)
                    .with_amendment(sepa::MandateAmendment::debtor_account_changed()),
                ),
            )
            .build()
            .unwrap();

        assert!(xml.contains("<AmdmntInd>true</AmdmntInd>"));
        assert!(
            xml.contains("<OrgnlDbtrAcct><Id><Othr><Id>SMNDA</Id></Othr></Id></OrgnlDbtrAcct>")
        );
        assert!(xml.contains("<UltmtCdtr><Nm>Netzbetreiber AG</Nm></UltmtCdtr>"));
        assert!(xml.contains("<UltmtDbtr><Nm>Erika Mustermann</Nm></UltmtDbtr>"));
        assert!(xml.contains("<Purp><Cd>ELEC</Cd></Purp>"));
        assert_validates(&xml, "pain.008.001.08.xsd");
    }

    #[test]
    fn creditor_id_amendment_validates_in_both_schemas() {
        let build = |schema| {
            let old = sepa::validate_creditor_id("DE98ZZZ09999999999").unwrap();
            super::Pain008Builder::new("Stadtwerke GmbH")
                .schema(schema)
                .msg_id("DD-CI-CHG")
                .add_group(
                    super::DirectDebitGroup::new(
                        "Stadtwerke GmbH",
                        &super::debtor(),
                        super::creditor_id(),
                    )
                    .collection_date("2026-07-20")
                    .add_entry(
                        super::DirectDebitEntry::new(
                            "MND-1",
                            "2024-06-01",
                            "Max",
                            super::creditor(),
                            100,
                            "E2E-1",
                        )
                        .with_amendment(
                            sepa::MandateAmendment::creditor_id_changed(old)
                                .with_original_creditor_name("Stadtwerke Muster GmbH"),
                        ),
                    ),
                )
                .build()
                .unwrap()
        };

        let v8 = build(DirectDebitSchema::IsoV8);
        assert!(v8.contains("<OrgnlCdtrSchmeId>"));
        assert!(v8.contains("<Prtry>SEPA</Prtry>"));
        assert!(!v8.contains("SMNDA"));
        assert_validates(&v8, "pain.008.001.08.xsd");
        assert_validates(&build(DirectDebitSchema::DkV2_7), "pain.008.003.02.xsd");
    }

    #[test]
    fn sct_instant_validates() {
        // Regression: SCT Inst uses pain.001.001.09, whose ReqdExctnDt is a
        // DateAndDateTime2Choice. A bare date there failed schema validation.
        let xml = super::Pain001Builder::new("Acme GmbH")
            .msg_id("CT-INST-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor())
                    .local_instrument(sepa::pain001::LocalInstrument::Inst)
                    .execution_date("2026-07-20")
                    .add_entry(super::CreditTransferEntry::new(
                        "Payee",
                        super::creditor(),
                        5_000,
                        "INST-001",
                    )),
            )
            .build()
            .unwrap();

        assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
        assert!(xml.contains("<ReqdExctnDt><Dt>2026-07-20</Dt></ReqdExctnDt>"));
        assert_validates(&xml, "pain.001.001.09.xsd");
    }

    #[test]
    fn a_bare_execution_date_would_not_validate_under_v9() {
        // Pins the reason the <Dt> wrapper exists: without it the file is
        // schema-invalid, so this must keep failing.
        let xml =
            sct(CreditTransferSchema::IsoV9).replace("<ReqdExctnDt><Dt>", "<ReqdExctnDt><WRONG>");
        assert!(
            !matches!(xmllint(&xml, "pain.001.001.09.xsd"), Some(Ok(()))),
            "a malformed ReqdExctnDt must not validate"
        );
    }
}

// ── generated documents satisfy the EPC rules the XSD does not ────────────────

#[test]
fn agents_never_use_notprovided_as_a_bic() {
    // `NOTPROVIDED` satisfies the BIC regex, so the XSD accepts
    // <BICFI>NOTPROVIDED</BICFI> — banks do not. The EPC "IBAN only" form is
    // <Othr><Id>NOTPROVIDED</Id></Othr>.
    let sct_xml = Pain001Builder::new("Acme GmbH")
        .msg_id("CT-NP")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor())
                .execution_date("2026-07-20")
                .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
        )
        .build()
        .unwrap();
    let sdd_xml = Pain008Builder::new("Stadtwerke GmbH")
        .msg_id("DD-NP")
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &debtor(), creditor_id())
                .collection_date("2026-07-20")
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    "2024-06-01",
                    "Max",
                    creditor(),
                    100,
                    "E2E-1",
                )),
        )
        .build()
        .unwrap();

    for xml in [&sct_xml, &sdd_xml] {
        assert!(!xml.contains("NOTPROVIDED</BIC>"));
        assert!(!xml.contains("NOTPROVIDED</BICFI>"));
    }
    assert!(!sct_xml.contains("<CdtrAgt>"));
    assert!(sct_xml.contains("<DbtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
    assert!(sdd_xml.contains("<CdtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
    assert!(sdd_xml.contains("<DbtrAgt><FinInstnId><Othr><Id>NOTPROVIDED</Id></Othr>"));
}

#[test]
fn payment_info_id_stays_within_max35text() {
    // Regression: PmtInfId used to be `MsgId + "-1"`, so a 35-character MsgId
    // produced a 37-character PmtInfId that breached Max35Text.
    let msg_id = "M".repeat(35);

    let sct_xml = Pain001Builder::new("Acme GmbH")
        .msg_id(&msg_id)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor())
                .execution_date("2026-07-20")
                .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
        )
        .build()
        .unwrap();

    let sdd_xml = Pain008Builder::new("Stadtwerke GmbH")
        .msg_id(&msg_id)
        .add_group(
            DirectDebitGroup::new("Stadtwerke GmbH", &debtor(), creditor_id())
                .collection_date("2026-07-20")
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    "2024-06-01",
                    "Max",
                    creditor(),
                    100,
                    "E2E-1",
                )),
        )
        .build()
        .unwrap();

    for xml in [&sct_xml, &sdd_xml] {
        let id = xml
            .split("<PmtInfId>")
            .nth(1)
            .unwrap()
            .split('<')
            .next()
            .unwrap();
        assert!(
            id.chars().count() <= 35,
            "PmtInfId {id:?} exceeds Max35Text"
        );
    }

    // An explicit override is validated the same way.
    assert!(matches!(
        Pain001Builder::new("Acme GmbH")
            .msg_id("SHORT")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &debtor())
                    .payment_info_id("P".repeat(36))
                    .execution_date("2026-07-20")
                    .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
            )
            .build(),
        Err(ValidationError::TooLong {
            field: "PmtInf/PmtInfId",
            ..
        })
    ));
}

#[test]
fn control_sum_and_counts_agree_at_both_levels() {
    let xml = sct(CreditTransferSchema::IsoV9);
    // 120.00 + 34.50; both GrpHdr and PmtInf carry the totals (EPC-mandatory
    // even though the XSD marks CtrlSum optional).
    assert_eq!(xml.matches("<NbOfTxs>2</NbOfTxs>").count(), 2);
    assert_eq!(xml.matches("<CtrlSum>154.50</CtrlSum>").count(), 2);
}

#[test]
fn every_emitted_text_value_is_in_the_sepa_character_set() {
    let xml = Pain008Builder::new("Müller & Söhne GmbH")
        .msg_id("DD-CHARSET")
        .add_group(
            DirectDebitGroup::new("Müller & Söhne GmbH", &debtor(), creditor_id())
                .collection_date("2026-07-20")
                .add_entry(
                    DirectDebitEntry::new(
                        "MND-1",
                        "2024-06-01",
                        "Jörg Groß",
                        creditor(),
                        100,
                        "E2E-1",
                    )
                    .with_description("Abschlag für Straße 1 — 100% fällig"),
                ),
        )
        .build()
        .unwrap();

    for tag in ["<Nm>", "<Ustrd>", "<MndtId>", "<EndToEndId>", "<MsgId>"] {
        for chunk in xml.split(tag).skip(1) {
            let value = chunk.split('<').next().unwrap();
            // Values are XML-escaped in the document; unescape before checking.
            let raw = value
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'");
            assert!(
                sepa::is_sepa_text(&raw),
                "{tag} value {raw:?} is not in the SEPA character set"
            );
        }
    }
}

#[test]
fn invalid_batches_are_rejected_before_any_xml_is_produced() {
    let group = || CreditTransferGroup::new("Acme GmbH", &debtor()).execution_date("2026-07-20");
    let base = || Pain001Builder::new("Acme GmbH").msg_id("CT-1");
    let entry = |ct| CreditTransferEntry::new("Payee", creditor(), ct, "E2E-1");

    assert_eq!(base().build(), Err(ValidationError::EmptyBatch));
    assert!(matches!(
        base().add_group(group().add_entry(entry(0))).build(),
        Err(ValidationError::AmountOutOfRange { .. })
    ));
    assert!(matches!(
        base()
            .add_group(group().add_entry(entry(100_000_000_000)))
            .build(),
        Err(ValidationError::AmountOutOfRange { .. })
    ));
    assert!(matches!(
        base()
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &debtor())
                    .execution_date("2026-02-30")
                    .add_entry(entry(100)),
            )
            .build(),
        Err(ValidationError::InvalidDate { .. })
    ));
}

// ── builder → parser round-trips ──────────────────────────────────────────────

#[test]
fn pain002_round_trip_reads_back_our_own_identifiers() {
    // Simulate the bank's response to the batch we just built and confirm the
    // identifiers survive the trip, including XML-escaped and umlaut text.
    let response = r#"<?xml version="1.0" encoding="UTF-8"?>
<ns2:Document xmlns:ns2="urn:iso:std:iso:20022:tech:xsd:pain.002.001.03">
  <ns2:CstmrPmtStsRpt>
    <ns2:GrpHdr>
      <ns2:MsgId>BANK-RESP-1</ns2:MsgId>
      <ns2:CreDtTm>2026-07-21T09:00:00</ns2:CreDtTm>
      <ns2:DbtrAgt><ns2:FinInstnId><ns2:BICFI>COBADEFFXXX</ns2:BICFI></ns2:FinInstnId></ns2:DbtrAgt>
    </ns2:GrpHdr>
    <ns2:OrgnlGrpInfAndSts>
      <ns2:OrgnlMsgId>CT-2026-07-001</ns2:OrgnlMsgId>
      <ns2:OrgnlMsgNmId>pain.001.001.09</ns2:OrgnlMsgNmId>
      <ns2:GrpSts>PART</ns2:GrpSts>
    </ns2:OrgnlGrpInfAndSts>
    <ns2:OrgnlPmtInfAndSts>
      <ns2:OrgnlPmtInfId>CT-2026-07-001-1</ns2:OrgnlPmtInfId>
      <!-- <ns2:TxInfAndSts><ns2:TxSts>IGNORED</ns2:TxSts></ns2:TxInfAndSts> -->
      <ns2:TxInfAndSts>
        <ns2:OrgnlEndToEndId>INV-2026-001</ns2:OrgnlEndToEndId>
        <ns2:TxSts>RJCT</ns2:TxSts>
        <ns2:StsRsnInf><ns2:Rsn><ns2:Cd>AC04</ns2:Cd></ns2:Rsn></ns2:StsRsnInf>
        <ns2:OrgnlTxRef>
          <ns2:Amt><ns2:InstdAmt Ccy="EUR">120.00</ns2:InstdAmt></ns2:Amt>
          <ns2:Cdtr><ns2:Nm>Bl&#252;mel &amp; S&#246;hne</ns2:Nm></ns2:Cdtr>
          <ns2:CdtrAcct><ns2:Id><ns2:IBAN>NL91ABNA0417164300</ns2:IBAN></ns2:Id></ns2:CdtrAcct>
        </ns2:OrgnlTxRef>
      </ns2:TxInfAndSts>
    </ns2:OrgnlPmtInfAndSts>
  </ns2:CstmrPmtStsRpt>
</ns2:Document>"#;

    let doc = parse_pain002(response).unwrap();
    assert_eq!(doc.original_msg_id, "CT-2026-07-001");
    assert!(doc.has_rejections());
    assert!(!doc.is_fully_accepted());
    // BICFI (2019 rename) must be read just like BIC.
    assert_eq!(doc.forwarding_agent_bic.as_deref(), Some("COBADEFFXXX"));

    let rejected = doc.rejected_transactions();
    assert_eq!(rejected.len(), 1, "the commented-out block must be ignored");
    let tx = rejected[0];
    assert_eq!(tx.original_end_to_end_id, "INV-2026-001");
    assert_eq!(tx.original_amount_ct, Some(12_000));
    // Entities and numeric character references are decoded.
    assert_eq!(tx.original_creditor_name.as_deref(), Some("Blümel & Söhne"));
    assert_eq!(
        tx.original_creditor_iban.as_deref(),
        Some("NL91ABNA0417164300")
    );
}

#[test]
fn camt053_batch_booking_exposes_every_transaction() {
    // A batch-booked direct debit collection: one aggregate entry, three
    // underlying transactions. Reconciliation needs all three.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>STMT-1</MsgId><CreDtTm>2026-07-21T23:59:00</CreDtTm></GrpHdr>
    <Stmt>
      <Id>2026-07-21</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Bal>
        <Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
        <Amt Ccy="EUR">1225.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Dt><Dt>2026-07-21</Dt></Dt>
      </Bal>
      <Ntry>
        <Amt Ccy="EUR">225.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Sts><Cd>BOOK</Cd></Sts>
        <BookgDt><Dt>2026-07-21</Dt></BookgDt>
        <BkTxCd><Domn><Cd>PMNT</Cd></Domn></BkTxCd>
        <NtryDtls>
          <TxDtls>
            <Amt Ccy="EUR">100.00</Amt>
            <Refs><EndToEndId>E2E-1</EndToEndId><MndtId>MND-1</MndtId></Refs>
            <RltdPties><Dbtr><Pty><Nm>Kunde Eins</Nm></Pty></Dbtr></RltdPties>
          </TxDtls>
          <TxDtls>
            <Amt Ccy="EUR">75.00</Amt>
            <Refs><EndToEndId>E2E-2</EndToEndId><MndtId>MND-2</MndtId></Refs>
            <RltdPties><Dbtr><Pty><Nm>Kunde Zwei</Nm></Pty></Dbtr></RltdPties>
          </TxDtls>
          <TxDtls>
            <Amt Ccy="EUR">50.00</Amt>
            <Refs><EndToEndId>E2E-3</EndToEndId><MndtId>MND-3</MndtId></Refs>
            <RtrInf><Rsn><Cd>MD01</Cd></Rsn></RtrInf>
          </TxDtls>
        </NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    let doc = parse_camt053(xml).unwrap();
    let stmt = &doc.statements[0];
    assert_eq!(stmt.account_iban, "DE89370400440532013000");
    assert_eq!(stmt.closing_balance().unwrap().signed_ct(), 122_500);

    let entry = &stmt.entries[0];
    assert_eq!(entry.amount_ct, 22_500);
    assert_eq!(entry.currency, "EUR");
    assert!(entry.batch_booked);
    // camt.053.001.08 wraps Sts in a Cd choice — v02 does not.
    assert_eq!(entry.status, sepa::EntryStatus::Booked);
    assert_eq!(entry.bank_tx_code.as_deref(), Some("PMNT"));

    assert_eq!(entry.details.len(), 3, "all three TxDtls must be kept");
    let ids: Vec<_> = entry
        .details
        .iter()
        .filter_map(|d| d.end_to_end_id.as_deref())
        .collect();
    assert_eq!(ids, ["E2E-1", "E2E-2", "E2E-3"]);
    // The details sum to the aggregate entry amount.
    let sum: i64 = entry.details.iter().filter_map(|d| d.amount_ct).sum();
    assert_eq!(sum, entry.amount_ct);
    // Party40Choice nesting (Dbtr/Pty/Nm) must resolve.
    assert_eq!(
        entry.details[0].counterparty_name.as_deref(),
        Some("Kunde Eins")
    );
    // A return anywhere in the batch is reported.
    assert!(entry.is_return());
    assert_eq!(entry.details[2].return_reason_code.as_deref(), Some("MD01"));
}

#[test]
fn camt053_v2_and_v8_shapes_parse_identically() {
    // The same logical statement in the two structurally different generations.
    let body = |sts: &str, dbtr: &str, ns: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.{ns}">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <Stmt>
      <Id>S1</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Ntry>
        <Amt Ccy="EUR">155.42</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        {sts}
        <NtryDtls><TxDtls>
          <Refs><EndToEndId>E2E-X</EndToEndId></Refs>
          <RltdPties>{dbtr}</RltdPties>
        </TxDtls></NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#
        )
    };

    let v2 = body("<Sts>BOOK</Sts>", "<Dbtr><Nm>Zahler</Nm></Dbtr>", "02");
    let v8 = body(
        "<Sts><Cd>BOOK</Cd></Sts>",
        "<Dbtr><Pty><Nm>Zahler</Nm></Pty></Dbtr>",
        "08",
    );

    for xml in [&v2, &v8] {
        let e = &parse_camt053(xml).unwrap().statements[0].entries[0];
        assert_eq!(e.status, sepa::EntryStatus::Booked);
        assert_eq!(e.signed_ct(), 15_542);
        assert_eq!(e.counterparty_name(), Some("Zahler"));
        assert_eq!(e.end_to_end_id(), Some("E2E-X"));
    }
}

#[test]
fn malformed_and_hostile_xml_is_rejected_not_mis_parsed() {
    // Not well-formed.
    assert!(parse_camt053("<Document><BkToCstmrStmt></Document>").is_err());
    // DOCTYPE is refused outright — defence in depth against entity expansion.
    let billion_laughs = r#"<!DOCTYPE lolz [<!ENTITY lol "lol">
        <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">]>
        <Document><BkToCstmrStmt><GrpHdr><MsgId>&lol2;</MsgId></GrpHdr></BkToCstmrStmt></Document>"#;
    assert!(parse_camt053(billion_laughs).is_err());
    // Not the expected message type.
    assert!(parse_pain002("<Document><BkToCstmrStmt/></Document>").is_err());
}

#[test]
fn a_multibyte_amount_is_rejected_not_a_panic() {
    // Regression: a `&#8364;` inside an amount decoded to '€' and then hit a
    // byte-index slice, panicking the process on a bank-supplied file.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <Stmt>
      <Id>S1</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Ntry>
        <Amt Ccy="EUR">1.&#8364;5</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    // The entry is dropped as unparseable; the parser must not panic.
    let doc = parse_camt053(xml).unwrap();
    assert!(doc.statements[0].entries.is_empty());
}

#[test]
fn a_second_root_element_cannot_displace_the_first() {
    // Document-substitution hazard: appending a second <Document> must not make
    // this library read different data than a validator saw. xmllint rejects
    // such input outright, and so must we.
    let one = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.03">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>REAL</MsgId><CreDtTm>T</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts><OrgnlMsgId>O-REAL</OrgnlMsgId><GrpSts>ACTC</GrpSts></OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
    let evil = one.replace("REAL", "EVIL");

    assert!(parse_pain002(one).is_ok());
    assert!(
        parse_pain002(&format!("{one}{evil}")).is_err(),
        "a trailing second root must be rejected, not silently win"
    );
}

#[test]
fn streaming_output_matches_the_in_memory_build() {
    let build = || {
        Pain008Builder::new("Stadtwerke GmbH")
            .msg_id("DD-STREAM")
            .created_at("2026-07-19T12:00:00")
            .add_group(
                DirectDebitGroup::new("Stadtwerke GmbH", &debtor(), creditor_id())
                    .collection_date("2026-07-20")
                    .add_entry(DirectDebitEntry::new(
                        "MND-1",
                        "2024-06-01",
                        "Max Mustermann",
                        creditor(),
                        7_500,
                        "E2E-1",
                    )),
            )
    };

    let direct = build().build().unwrap();
    let mut via_io: Vec<u8> = Vec::new();
    build().write_to(&mut via_io).unwrap();
    assert_eq!(direct, String::from_utf8(via_io).unwrap());

    // The streaming path validates too, and writes nothing when it refuses.
    let mut empty: Vec<u8> = Vec::new();
    let err = Pain008Builder::new("Stadtwerke GmbH")
        .write_to(&mut empty)
        .unwrap_err();
    assert!(matches!(err, sepa::WriteError::Validation(_)));
    assert!(empty.is_empty(), "a rejected batch must write nothing");
}

#[test]
fn large_batch_totals_stay_exact() {
    // 10 000 transactions of 1 ct each: integer arithmetic must land on exactly
    // 100.00 EUR, where repeated f64 addition would drift.
    let entries =
        (0..10_000).map(|i| CreditTransferEntry::new("Payee", creditor(), 1, format!("E2E-{i}")));

    let builder = Pain001Builder::new("Acme GmbH").msg_id("BULK").add_group(
        CreditTransferGroup::new("Acme GmbH", &debtor())
            .execution_date("2026-07-20")
            .add_entries(entries),
    );

    assert_eq!(builder.entry_count(), 10_000);
    assert_eq!(builder.total_ct(), 10_000);

    let xml = builder.build().unwrap();
    assert!(xml.contains("<NbOfTxs>10000</NbOfTxs>"));
    assert!(xml.contains("<CtrlSum>100.00</CtrlSum>"));
}
