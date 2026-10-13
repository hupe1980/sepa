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
    BuildError, CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup,
    Pain001Builder, Pain008Builder, ValidationError, parse_camt053, parse_pain002,
    validate_creditor_id, validate_iban,
};

// ── fixtures ──────────────────────────────────────────────────────────────────

fn debtor() -> sepa::Iban {
    validate_iban("DE89370400440532013000").unwrap()
}

fn creditor() -> sepa::Iban {
    validate_iban("NL91ABNA0417164300").unwrap()
}

fn date(s: &str) -> sepa::IsoDate {
    s.parse().unwrap()
}

fn creditor_id() -> sepa::CreditorId {
    validate_creditor_id("DE98ZZZ09999999999").unwrap()
}

fn sct(schema: CreditTransferSchema) -> String {
    Pain001Builder::new("Acme GmbH", "CT-2026-07-001-MAXLEN-PADDING-XXXXX")
        .schema(schema)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20"))
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
    Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
        .schema(schema)
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke GmbH",
                &debtor(),
                &creditor_id(),
                date("2026-07-20"),
            )
            .creditor_bic("COBADEFF".parse().unwrap())
            .add_entry(
                DirectDebitEntry::new(
                    "MND-00042",
                    date("2024-06-01"),
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

/// Every camt.055 shape the builder can produce, for the schema gate.
///
/// Shared with nothing else on purpose: these are the documents the XSD sees,
/// and a scope that is not in this list is a scope no schema has checked.
fn camt055_fixtures() -> Vec<String> {
    use sepa::{
        Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason, OriginalMessage,
        validate_bic,
    };

    let submitted = Pain008Builder::new("Stadtwerke GmbH", "DD-2026-07-001")
        .created_at("2026-07-15T09:00:00".parse().unwrap())
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke GmbH",
                &creditor(),
                &creditor_id(),
                date("2026-07-20"),
            )
            .payment_info_id("PMT-A")
            .add_entry(DirectDebitEntry::new(
                "MND-1",
                date("2024-06-01"),
                "Max Mustermann",
                debtor(),
                7_500,
                "E2E-1",
            )),
        );
    let base = || {
        Camt055Builder::new(
            "CXL-2026-07-001",
            "Stadtwerke GmbH",
            validate_bic("COBADEFFXXX").unwrap(),
            OriginalMessage::from_direct_debit(&submitted),
        )
        .created_at("2026-07-15T11:30:00".parse().unwrap())
    };

    vec![
        // Named transactions, with every optional element populated.
        base()
            .case_id("CASE-2026-07-001")
            .add_group(
                CancellationGroup::new("PMT-A")
                    .payment_cancellation_id("PC-1")
                    .add_entry(
                        CancellationEntry::new("E2E-1", CancellationReason::Dupl)
                            .cancellation_id("CX-1")
                            .original_instruction_id("INSTR-1")
                            .original_amount(7_500)
                            .additional_info("Doppelte Einreichung"),
                    ),
            )
            .build()
            .unwrap(),
        // A whole `PmtInf`.
        base()
            .add_group(
                CancellationGroup::new("PMT-A")
                    .cancel_whole_group(CancellationReason::Upay)
                    .additional_info("Lauf zurueckgezogen"),
            )
            .build()
            .unwrap(),
        // The whole file.
        base()
            .cancel_whole_message(CancellationReason::Tech)
            .additional_info("Fehlerhafter Lauf")
            .build()
            .unwrap(),
        // A proprietary reason, which must land in `Prtry` and not in `Cd`.
        base()
            .add_group(
                CancellationGroup::new("PMT-A").add_entry(CancellationEntry::new(
                    "E2E-1",
                    "XY99".parse::<sepa::CancellationReason>().unwrap(),
                )),
            )
            .build()
            .unwrap(),
    ]
}

/// The camt.029 fixtures the parser unit tests read, for the schema gate.
fn camt029_fixtures() -> Vec<String> {
    let accepted = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-1</Id>
      <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
      <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
      <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-1</Id><Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
    <Sts><Conf>CNCL</Conf></Sts>
    <CxlDtls>
      <OrgnlGrpInfAndSts>
        <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
        <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
        <OrgnlCreDtTm>2026-07-15T09:00:00</OrgnlCreDtTm>
        <OrgnlNbOfTxs>3</OrgnlNbOfTxs>
        <OrgnlCtrlSum>225.00</OrgnlCtrlSum>
      </OrgnlGrpInfAndSts>
      <OrgnlPmtInfAndSts>
        <OrgnlPmtInfCxlId>PC-1</OrgnlPmtInfCxlId>
        <OrgnlPmtInfId>PMT-A</OrgnlPmtInfId>
        <NbOfTxsPerCxlSts><DtldNbOfTxs>1</DtldNbOfTxs><DtldSts>ACCR</DtldSts>
          <DtldCtrlSum>75.00</DtldCtrlSum></NbOfTxsPerCxlSts>
        <TxInfAndSts>
          <CxlStsId>CS-1</CxlStsId>
          <OrgnlEndToEndId>E2E-1</OrgnlEndToEndId>
          <TxCxlSts>ACCR</TxCxlSts>
          <OrgnlInstdAmt Ccy="EUR">75.00</OrgnlInstdAmt>
          <OrgnlReqdColltnDt>2026-07-20</OrgnlReqdColltnDt>
        </TxInfAndSts>
      </OrgnlPmtInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn>
</Document>"#;

    let refused = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.029.001.06">
  <RsltnOfInvstgtn>
    <Assgnmt><Id>RES-2</Id>
      <Assgnr><Agt><FinInstnId><BICFI>COBADEFFXXX</BICFI></FinInstnId></Agt></Assgnr>
      <Assgne><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Assgne>
      <CreDtTm>2026-07-15T14:02:00</CreDtTm></Assgnmt>
    <RslvdCase><Id>CXL-2</Id><Cretr><Pty><Nm>Stadtwerke GmbH</Nm></Pty></Cretr></RslvdCase>
    <Sts><Conf>RJCR</Conf></Sts>
    <CxlDtls>
      <OrgnlGrpInfAndSts>
        <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
        <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
        <GrpCxlSts>RJCR</GrpCxlSts>
        <CxlStsRsnInf><Rsn><Cd>ARDT</Cd></Rsn>
          <AddtlInf>Bereits ausgefuehrt und zurueckgegeben</AddtlInf>
          <AddtlInf>Bitte pain.007 verwenden</AddtlInf></CxlStsRsnInf>
      </OrgnlGrpInfAndSts>
    </CxlDtls>
  </RsltnOfInvstgtn>
</Document>"#;

    vec![accepted.to_owned(), refused.to_owned()]
}

// ── XSD validation ────────────────────────────────────────────────────────────

mod xsd {
    use super::{
        Command, CreditTransferSchema, DirectDebitSchema, ValidationError, date, sct, sdd,
    };

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

    /// The XSD file that corresponds to a schema variant.
    fn schema_file(message_id: &str) -> String {
        format!("{message_id}.xsd")
    }

    #[test]
    fn cancellation_reason_preserves_text_without_injecting_markup() {
        let payload = "A\r\nB\rC <Injected/> &#13;";
        let builder = sepa::Camt055Builder::new(
            "CXL-ESCAPE",
            "Acme GmbH",
            "COBADEFFXXX".parse::<sepa::Bic>().unwrap(),
            sepa::OriginalMessage::new("ORIGINAL", "pain.008.001.08"),
        )
        .created_at("2026-07-15T11:30:00".parse().unwrap())
        .cancel_whole_message(sepa::CancellationReason::Tech)
        .additional_info(payload);
        let xml = builder.build().unwrap();
        let expected =
            sepa::charset::transliterate(payload, sepa::charset::Transliteration::German);
        assert!(xml.contains(&format!("<AddtlInf>{expected}</AddtlInf>")));
        assert!(!xml.contains("<Injected/>"));
        let mut streamed = Vec::new();
        builder.write_to(&mut streamed).unwrap();
        assert_eq!(streamed, xml.as_bytes());
        assert_eq!(xmllint(&xml, "camt.055.001.05.xsd"), Some(Ok(())));
        assert!(
            builder
                .charset(sepa::CharsetPolicy::Strict)
                .build()
                .is_err()
        );
    }

    #[test]
    fn cancellation_message_id_refuses_invalid_xml_and_padded_overlength_before_output() {
        for payload in ["A\0B".to_owned(), format!(" {} ", "A".repeat(34))] {
            let builder = sepa::Camt055Builder::new(
                payload.clone(),
                "Acme GmbH",
                "COBADEFFXXX".parse::<sepa::Bic>().unwrap(),
                sepa::OriginalMessage::new("ORIGINAL", "pain.008.001.08"),
            )
            .cancel_whole_message(sepa::CancellationReason::Tech);
            assert!(builder.build().is_err(), "{payload:?}");
            let mut streamed = b"existing output".to_vec();
            assert!(builder.write_to(&mut streamed).is_err(), "{payload:?}");
            assert_eq!(streamed, b"existing output");
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn every_writer_keeps_external_text_out_of_xml_structure() {
        use super::{creditor, creditor_id, debtor};
        use sepa::{
            CreditTransferEntry, CreditTransferGroup, DirectDebitEntry, DirectDebitGroup,
            Pain001Builder, Pain008Builder,
        };

        fn structure(xml: &str) -> Vec<Vec<u8>> {
            let mut reader = quick_xml::Reader::from_str(xml);
            let mut shape = Vec::new();
            loop {
                match reader.read_event().unwrap() {
                    quick_xml::events::Event::Start(tag) => {
                        shape.push([b"+".as_slice(), tag.name().as_ref()].concat());
                    }
                    quick_xml::events::Event::End(tag) => {
                        shape.push([b"-".as_slice(), tag.name().as_ref()].concat());
                    }
                    quick_xml::events::Event::Empty(tag) => {
                        shape.push([b"+".as_slice(), tag.name().as_ref()].concat());
                        shape.push([b"-".as_slice(), tag.name().as_ref()].concat());
                    }
                    quick_xml::events::Event::Eof => break,
                    quick_xml::events::Event::DocType(_) => panic!("writer emitted a DTD"),
                    _ => {}
                }
            }
            shape
        }
        fn documents(text: &str) -> Vec<(&'static str, String)> {
            let mut documents = Vec::new();
            let identifier = if text == "SAFE" { "SAFE" } else { "X'1" };
            macro_rules! record {
                ($builder:expr, $schema:literal) => {{
                    let builder = $builder.created_at("2026-07-19T12:00:00Z".parse().unwrap());
                    let xml = builder.build().unwrap();
                    let mut sink = b"existing".to_vec();
                    super::assert_writer_boundary(
                        Ok(xml.clone()),
                        builder.write_to(&mut sink),
                        &sink,
                        None,
                        "<Document",
                    );
                    documents.push(($schema, xml));
                }};
            }
            let address = sepa::PostalAddress::new(text, "DE")
                .unwrap()
                .street(text)
                .building_number(text)
                .post_code(text)
                .country_subdivision(text)
                .department(text)
                .sub_department(text)
                .line(text);
            let party = sepa::Party::new(text).with_organisation_id(identifier, Some(text));
            for proprietary_remittance in [false, true] {
                let remittance = if proprietary_remittance {
                    sepa::RemittanceInfo::Proprietary {
                        reference: identifier.into(),
                        issuer: Some(text.into()),
                    }
                } else {
                    sepa::RemittanceInfo::unstructured(text)
                };
                for group_party in [false, true] {
                    let mut ct_entry = CreditTransferEntry::new(text, creditor(), 100, identifier)
                        .with_creditor_address(address.clone())
                        .with_ultimate_creditor(party.clone());
                    if !group_party {
                        ct_entry = ct_entry.with_ultimate_debtor(party.clone());
                    }
                    ct_entry.remittance = Some(remittance.clone());
                    let ct_group = CreditTransferGroup::new(text, &debtor(), date("2026-07-20"))
                        .payment_info_id(identifier)
                        .debtor_address(address.clone());
                    let ct_group = if group_party {
                        ct_group.ultimate_debtor(party.clone())
                    } else {
                        ct_group
                    };
                    record!(
                        Pain001Builder::new(text, identifier)
                            .add_group(ct_group.add_entry(ct_entry)),
                        "pain.001.001.09.xsd"
                    );
                    let mut dd_entry = DirectDebitEntry::new(
                        identifier,
                        date("2024-06-01"),
                        text,
                        debtor(),
                        100,
                        identifier,
                    )
                    .with_debtor_address(address.clone())
                    .with_ultimate_debtor(party.clone())
                    .with_amendment(
                        sepa::MandateAmendment::creditor_name_changed(text)
                            .with_original_mandate_id(identifier),
                    );
                    if !group_party {
                        dd_entry = dd_entry.with_ultimate_creditor(party.clone());
                    }
                    dd_entry.remittance = Some(remittance.clone());
                    let dd_group = DirectDebitGroup::new(
                        text,
                        &creditor(),
                        &creditor_id(),
                        date("2026-07-20"),
                    )
                    .payment_info_id(identifier)
                    .creditor_address(address.clone());
                    let dd_group = if group_party {
                        dd_group.ultimate_creditor(party.clone())
                    } else {
                        dd_group
                    };
                    record!(
                        Pain008Builder::new(text, identifier)
                            .add_group(dd_group.add_entry(dd_entry)),
                        "pain.008.001.08.xsd"
                    );
                }
            }
            let original = sepa::OriginalCollection::new(identifier, date("2024-06-01"))
                .debtor(text, debtor())
                .creditor(text, creditor());
            record!(
                sepa::Pain007Builder::new(text, identifier, identifier).add_group(
                    sepa::ReversalGroup::new(identifier)
                        .reversal_payment_info_id(identifier)
                        .add_entry(sepa::ReversalEntry::new(
                            identifier,
                            100,
                            sepa::ReversalReason::Other("ABCD".to_owned()),
                            original
                        ))
                ),
                "pain.007.001.09.xsd"
            );
            for scope in 0..3 {
                let builder = sepa::Camt055Builder::new(
                    identifier,
                    party.clone(),
                    party.clone(),
                    sepa::OriginalMessage::new(identifier, identifier),
                )
                .case_id(identifier);
                let builder = match scope {
                    0 => builder
                        .cancel_whole_message(sepa::CancellationReason::Other("ABCD".to_owned()))
                        .additional_info(text),
                    1 => builder.add_group(
                        sepa::CancellationGroup::new(identifier)
                            .payment_cancellation_id(identifier)
                            .cancel_whole_group(sepa::CancellationReason::Cust)
                            .additional_info(text),
                    ),
                    _ => builder.add_group(
                        sepa::CancellationGroup::new(identifier)
                            .payment_cancellation_id(identifier)
                            .add_entry(
                                sepa::CancellationEntry::new(
                                    identifier,
                                    sepa::CancellationReason::Cust,
                                )
                                .cancellation_id(identifier)
                                .original_instruction_id(identifier)
                                .additional_info(text),
                            ),
                    ),
                };
                record!(builder, "camt.055.001.05.xsd");
            }
            documents
        }
        let baseline = documents("SAFE");
        let payload = "X<&'\">";
        let converted =
            sepa::charset::transliterate(payload, sepa::charset::Transliteration::German);
        let hostile = documents(payload);
        assert_eq!(baseline.len(), 12);
        assert_eq!(hostile.len(), baseline.len());
        for ((schema, baseline), (hostile_schema, xml)) in baseline.into_iter().zip(hostile) {
            assert_eq!(schema, hostile_schema);
            assert_eq!(structure(&baseline), structure(&xml), "{schema}");
            let plain = super::text_nodes_with_path(&baseline)
                .into_iter()
                .filter(|node| node.2 == "SAFE")
                .count();
            let protected = super::text_nodes_with_path(&xml)
                .into_iter()
                .filter(|node| node.2 == "X'1" || node.2 == converted)
                .count();
            assert!(plain > 5, "empty text-position control for {schema}");
            assert_eq!(
                plain, protected,
                "lost or structurally interpreted text in {schema}"
            );
            assert_eq!(xmllint(&xml, schema), Some(Ok(())), "{schema}");
        }
    }

    #[test]
    fn lawful_original_recall_totals_validate_against_the_pinned_schema() {
        for (count, amount) in [
            (0, -100),
            (999_999_999_999_999, 999_999_999_999_999_999),
            (1, 9_223_372_036_854_775_800),
        ] {
            let xml = sepa::Camt055Builder::new(
                "RECALL-1",
                "Customer",
                "Bank",
                sepa::OriginalMessage::new("ORIGINAL-1", "pain.008.001.08").totals(count, amount),
            )
            .created_at("2026-07-19T12:00:00Z".parse().unwrap())
            .cancel_whole_message(sepa::CancellationReason::Cust)
            .build()
            .unwrap();
            assert_eq!(xmllint(&xml, "camt.055.001.05.xsd"), Some(Ok(())));
        }
    }

    #[test]
    fn every_pain001_schema_version_validates() {
        // One test per variant would drift the moment a variant is added; this
        // fails to compile-and-cover nothing if `ALL` grows.
        for schema in CreditTransferSchema::ALL {
            assert_validates(&sct(*schema), &schema_file(schema.message_id()));
        }
    }

    #[test]
    fn every_pain008_schema_version_validates() {
        for schema in DirectDebitSchema::ALL {
            assert_validates(&sdd(*schema), &schema_file(schema.message_id()));
        }
    }

    #[test]
    fn agentless_batches_validate_in_every_schema_version() {
        // The EPC "IBAN only" form is a different element in each generation,
        // and a batch with no BIC at all is the common case for a German
        // creditor — so it needs covering per version, not just by default.
        for schema in CreditTransferSchema::ALL {
            let xml = super::Pain001Builder::new("Acme GmbH", "CT-NOBIC")
                .schema(*schema)
                .add_group(
                    super::CreditTransferGroup::new(
                        "Acme GmbH",
                        &super::debtor(),
                        date("2026-07-20"),
                    )
                    .add_entry(super::CreditTransferEntry::new(
                        "Payee",
                        super::creditor(),
                        100,
                        "E2E-1",
                    )),
                )
                .build()
                .unwrap();
            assert!(!xml.contains("<CdtrAgt>"), "{schema}: EPC omits CdtrAgt");
            assert_validates(&xml, &schema_file(schema.message_id()));
        }

        for schema in DirectDebitSchema::ALL {
            let xml = super::Pain008Builder::new("Stadtwerke GmbH", "DD-NOBIC")
                .schema(*schema)
                .add_group(
                    super::DirectDebitGroup::new(
                        "Stadtwerke GmbH",
                        &super::debtor(),
                        &super::creditor_id(),
                        date("2026-07-20"),
                    )
                    .add_entry(super::DirectDebitEntry::new(
                        "MND-1",
                        date("2024-06-01"),
                        "Max Mustermann",
                        super::creditor(),
                        7_500,
                        "E2E-1",
                    )),
                )
                .build()
                .unwrap();
            assert!(
                xml.contains("<Othr><Id>NOTPROVIDED</Id></Othr>"),
                "{schema}"
            );
            assert_validates(&xml, &schema_file(schema.message_id()));
        }
    }

    #[test]
    fn the_smnda_marker_validates_in_every_schema_version() {
        // Regression: pain.008.003.02 permits nothing but an IBAN under
        // OrgnlDbtrAcct and enumerates SMNDA under OrgnlDbtrAgt instead, so the
        // post-2016 placement was schema-invalid there.
        for schema in DirectDebitSchema::ALL {
            let xml = super::Pain008Builder::new("Stadtwerke GmbH", "DD-SMNDA")
                .schema(*schema)
                .add_group(
                    super::DirectDebitGroup::new(
                        "Stadtwerke GmbH",
                        &super::debtor(),
                        &super::creditor_id(),
                        date("2026-07-20"),
                    )
                    .add_entry(
                        super::DirectDebitEntry::new(
                            "MND-1",
                            date("2024-06-01"),
                            "Max Mustermann",
                            super::creditor(),
                            7_500,
                            "E2E-1",
                        )
                        .with_amendment(sepa::MandateAmendment::debtor_account_changed()),
                    ),
                )
                .build()
                .unwrap();
            assert!(xml.contains("SMNDA"), "{schema}");
            assert_validates(&xml, &schema_file(schema.message_id()));
        }
    }

    /// OCT Inst output, measured against the ISO schema rather than against
    /// this crate's own opinion of it.
    ///
    /// The scheme adds no message — EPC250-22 specifies `pain.001.001.09`, the
    /// version already emitted here — so the only question a schema can answer
    /// is whether the four elements it *does* change still sit in a document
    /// `CustomerCreditTransferInitiationV09` admits. `InstrForCdtrAgt` in
    /// particular has to land between `UltmtCdtr` and `Purp`, which is the
    /// class of mistake that produced D43.
    #[test]
    fn oct_inst_validates_against_the_iso_schema() {
        let xml = super::Pain001Builder::new("Acme GmbH", "OCT-XSD")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), date("2026-07-20"))
                    .kind(sepa::pain001::CreditTransferKind::OneLegOutInstant)
                    .charge_bearer(sepa::pain001::ChargeBearer::Shar)
                    .add_entry(
                        super::CreditTransferEntry::new("Payee", super::creditor(), 5_000, "OCT-1")
                            .with_currency("USD".parse().unwrap())
                            .with_non_euro_leg_currency("USD".parse().unwrap())
                            .with_purpose("RRTP".parse().unwrap())
                            .with_description("one leg out"),
                    ),
            )
            .build()
            .unwrap();

        assert!(xml.contains("<SvcLvl><Cd>EOLO</Cd></SvcLvl>"), "{xml}");
        assert!(
            xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"),
            "{xml}"
        );
        assert!(xml.contains("<ChrgBr>SHAR</ChrgBr>"), "{xml}");
        assert!(
            xml.contains(r#"<InstdAmt Ccy="USD">50.00</InstdAmt>"#),
            "{xml}"
        );
        assert!(
            xml.contains("<InstrForCdtrAgt><InstrInf>USD</InstrInf></InstrForCdtrAgt>"),
            "{xml}"
        );
        // Element order is the thing only a schema can check.
        let instr = xml.find("<InstrForCdtrAgt>").expect("AT-T020 present");
        let ultmt = xml.find("<UltmtCdtr>").unwrap_or(0);
        let purp = xml.find("<Purp>").expect("purpose present");
        assert!(
            ultmt <= instr && instr < purp,
            "InstrForCdtrAgt is misplaced"
        );

        assert_validates(&xml, &schema_file("pain.001.001.09"));
    }

    #[test]
    fn sct_instant_validates_in_every_schema_version_that_allows_it() {
        // Regression: the DK schema has no LclInstrm element, and the builder
        // used to emit one anyway — producing a file that failed its own XSD.
        for schema in CreditTransferSchema::ALL {
            let built = super::Pain001Builder::new("Acme GmbH", "CT-INST")
                .schema(*schema)
                .add_group(
                    super::CreditTransferGroup::new(
                        "Acme GmbH",
                        &super::debtor(),
                        date("2026-07-20"),
                    )
                    .kind(sepa::pain001::CreditTransferKind::Instant)
                    .add_entry(super::CreditTransferEntry::new(
                        "Payee",
                        super::creditor(),
                        5_000,
                        "INST-1",
                    )),
                )
                .build();

            if schema.supports_local_instrument() {
                let xml = built.expect("a schema with LclInstrm accepts INST");
                assert!(xml.contains("<LclInstrm><Cd>INST</Cd></LclInstrm>"));
                assert_validates(&xml, &schema_file(schema.message_id()));
            } else {
                assert_eq!(
                    built.unwrap_err().kind,
                    ValidationError::UnsupportedBySchema {
                        feature: "PmtTpInf/LclInstrm (SCT Inst)",
                        schema: schema.message_id(),
                    },
                    "{schema} has no LclInstrm and must refuse rather than emit one",
                );
            }
        }
    }

    #[test]
    fn postal_addresses_validate_in_every_schema_that_has_them() {
        // `PostalAddress6` (pain.001.001.03) and `PostalAddress24`
        // (pain.001.001.09) share the elements this crate emits and their
        // order, so one address value has to validate against both. The DK
        // schema's `PostalAddressSEPA` holds only Ctry and two AdrLines, so it
        // must refuse rather than emit something its own XSD rejects.
        let address = || {
            sepa::PostalAddress::new("Berlin", "DE")
                .unwrap()
                .street("Unter den Linden")
                .building_number("77")
                .post_code("10117")
                .country_subdivision("BE")
        };

        for schema in CreditTransferSchema::ALL {
            let built = super::Pain001Builder::new("Acme GmbH", "CT-ADR")
                .schema(*schema)
                .add_group(
                    super::CreditTransferGroup::new(
                        "Acme GmbH",
                        &super::debtor(),
                        date("2026-07-20"),
                    )
                    .debtor_address(address())
                    .add_entry(
                        super::CreditTransferEntry::new(
                            "Supplier AG",
                            super::creditor(),
                            12_000,
                            "E2E-1",
                        )
                        .with_creditor_address(
                            sepa::PostalAddress::new("Amsterdam", "NL")
                                .unwrap()
                                .line("Herengracht 1"),
                        ),
                    ),
                )
                .build();

            if schema.supports_postal_address() {
                let xml = built.expect("an ISO schema carries PstlAdr");
                assert!(xml.contains("<PstlAdr><StrtNm>Unter den Linden</StrtNm>"));
                assert!(
                    xml.contains(
                        "<TwnNm>Berlin</TwnNm><CtrySubDvsn>BE</CtrySubDvsn><Ctry>DE</Ctry>"
                    )
                );
                assert!(xml.contains(
                    "<TwnNm>Amsterdam</TwnNm><Ctry>NL</Ctry><AdrLine>Herengracht 1</AdrLine>"
                ));
                assert_validates(&xml, &schema_file(schema.message_id()));
            } else {
                assert_eq!(
                    built.unwrap_err().kind,
                    ValidationError::UnsupportedBySchema {
                        feature: "Dbtr/PstlAdr",
                        schema: schema.message_id(),
                    },
                    "{schema} cannot hold a structured address",
                );
            }
        }

        for schema in DirectDebitSchema::ALL {
            let built = super::Pain008Builder::new("Stadtwerke GmbH", "DD-ADR")
                .schema(*schema)
                .add_group(
                    super::DirectDebitGroup::new(
                        "Stadtwerke GmbH",
                        &super::debtor(),
                        &super::creditor_id(),
                        date("2026-07-20"),
                    )
                    .creditor_address(address())
                    .add_entry(
                        super::DirectDebitEntry::new(
                            "MND-1",
                            date("2024-06-01"),
                            "Max Mustermann",
                            super::creditor(),
                            7_500,
                            "E2E-1",
                        )
                        .with_debtor_address(
                            sepa::PostalAddress::new("Wien", "AT")
                                .unwrap()
                                .post_code("1010"),
                        ),
                    ),
                )
                .build();

            if schema.supports_postal_address() {
                let xml = built.expect("an ISO schema carries PstlAdr");
                assert!(xml.contains("<PstCd>1010</PstCd><TwnNm>Wien</TwnNm><Ctry>AT</Ctry>"));
                assert_validates(&xml, &schema_file(schema.message_id()));
            } else {
                assert_eq!(
                    built.unwrap_err().kind,
                    ValidationError::UnsupportedBySchema {
                        feature: "Cdtr/PstlAdr",
                        schema: schema.message_id(),
                    },
                    "{schema} cannot hold a structured address",
                );
            }
        }
    }

    #[test]
    fn a_timed_execution_validates_only_where_the_choice_exists() {
        // The DK's "terminierte Echtzeitüberweisung": a scheduled instant
        // transfer due at a stated time, which needs ReqdExctnDt/DtTm. Only
        // pain.001.001.09 types ReqdExctnDt as a date/time choice.
        for schema in CreditTransferSchema::ALL {
            let built = super::Pain001Builder::new("Acme GmbH", "CT-TIMED")
                .schema(*schema)
                .add_group(
                    super::CreditTransferGroup::new(
                        "Acme GmbH",
                        &super::debtor(),
                        "2026-07-20T11:00:00Z".parse::<sepa::IsoDateTime>().unwrap(),
                    )
                    .kind(sepa::CreditTransferKind::Instant)
                    .add_entry(super::CreditTransferEntry::new(
                        "Payee",
                        super::creditor(),
                        5_000,
                        "E2E-1",
                    )),
                )
                .build();

            if schema.supports_execution_time() {
                let xml = built.expect("the choice type accepts a time");
                assert!(
                    xml.contains("<ReqdExctnDt><DtTm>2026-07-20T11:00:00Z</DtTm></ReqdExctnDt>")
                );
                assert_validates(&xml, &schema_file(schema.message_id()));
                assert_validates(&xml, "pain.001.001.09_GBIC_5.xsd");
            } else {
                // The DK schema has no LclInstrm either, so it fails on that
                // first — both are UnsupportedBySchema for the same reason.
                assert!(
                    matches!(
                        built.unwrap_err().kind,
                        ValidationError::UnsupportedBySchema { .. }
                    ),
                    "{schema} has a bare ISODate and must refuse a time",
                );
            }
        }
    }

    #[test]
    fn a_timed_execution_needs_sct_instant_and_a_utc_offset() {
        // The DK validation subset annotates ReqdExctnDt/DtTm "Only allowed for
        // SCTinst", with the usage rule "Only UTC time format or local time
        // with UTC offset format can be used". Neither is expressible in XSD,
        // so neither is caught by xmllint — a file breaking them validates
        // cleanly and is rejected on ingestion.
        let build = |instant: bool, moment: &str| {
            let moment: sepa::IsoDateTime = moment.parse().unwrap();
            let mut group = super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), moment);
            if instant {
                group = group.kind(sepa::CreditTransferKind::Instant);
            }
            super::Pain001Builder::new("Acme GmbH", "CT-TIMED")
                .add_group(group.add_entry(super::CreditTransferEntry::new(
                    "Payee",
                    super::creditor(),
                    5_000,
                    "E2E-1",
                )))
                .build()
        };

        assert_eq!(
            build(false, "2026-07-20T11:00:00Z").unwrap_err().kind,
            ValidationError::Requires {
                feature: "ReqdExctnDt/DtTm (timed execution)",
                requires: "an instant scheme — CreditTransferKind::Instant or ::OneLegOutInstant",
            },
        );
        assert_eq!(
            build(true, "2026-07-20T11:00:00").unwrap_err().kind,
            ValidationError::Requires {
                feature: "ReqdExctnDt/DtTm (timed execution)",
                requires: "a UTC offset — see IsoDateTime::in_utc",
            },
        );
        // Both forms the usage rule names are accepted.
        for moment in ["2026-07-20T11:00:00Z", "2026-07-20T13:00:00+02:00"] {
            let xml = build(true, moment).unwrap();
            assert!(xml.contains(&format!("<DtTm>{moment}</DtTm>")));
            assert_validates(&xml, "pain.001.001.09_GBIC_5.xsd");
        }
        // A plain date is unaffected — it is the ordinary SCT case.
        assert!(
            super::Pain001Builder::new("Acme GmbH", "CT-PLAIN")
                .add_group(
                    super::CreditTransferGroup::new(
                        "Acme GmbH",
                        &super::debtor(),
                        super::date("2026-07-20")
                    )
                    .add_entry(super::CreditTransferEntry::new(
                        "Payee",
                        super::creditor(),
                        5_000,
                        "E2E-1",
                    )),
                )
                .build()
                .is_ok()
        );
    }

    #[test]
    fn direct_debit_reversals_validate() {
        // pain.007 is validated against the DK GBIC technical validation
        // subset, which is a restriction of the ISO schema.
        let creditor = super::debtor();
        let ci = super::creditor_id();
        let group =
            super::DirectDebitGroup::new("Stadtwerke GmbH", &creditor, &ci, date("2026-07-20"))
                .sequence_type(sepa::SequenceType::Frst)
                .creditor_bic("COBADEFFXXX".parse().unwrap());
        let entry = super::DirectDebitEntry::new(
            "MND-42",
            date("2024-06-01"),
            "Max Mustermann",
            super::creditor(),
            7_500,
            "E2E-1",
        );

        // Full reference form.
        let xml = sepa::Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-001")
            .creditor_agent("COBADEFFXXX".parse().unwrap())
            .add_group(sepa::ReversalGroup::new("DD-2026-07-001").add_entry(
                sepa::ReversalEntry::reverse(&group, &entry, sepa::ReversalReason::Ms02),
            ))
            .build()
            .unwrap();
        assert_validates(&xml, "pain.007.001.09.xsd");

        // The minimum the DK subset accepts: OrgnlTxRef with just the mandate.
        let bare = sepa::Pain007Builder::new("Stadtwerke GmbH", "DD-2026-07-001", "RVSL-002")
            .add_group(sepa::ReversalGroup::new("DD-2026-07-001").add_entry(
                sepa::ReversalEntry::new(
                    "E2E-1",
                    7_500,
                    sepa::ReversalReason::Am05,
                    sepa::OriginalCollection::new("MND-42", date("2024-06-01")),
                ),
            ))
            .build()
            .unwrap();
        assert!(bare.contains("<MndtId>MND-42</MndtId>"));
        assert_validates(&bare, "pain.007.001.09.xsd");

        // A partial reversal is still a valid document.
        let partial = sepa::Pain007Builder::new("Stadtwerke GmbH", "DD-1", "RVSL-003")
            .add_group(
                sepa::ReversalGroup::new("DD-1").add_entry(
                    sepa::ReversalEntry::reverse(&group, &entry, sepa::ReversalReason::Ms02)
                        .reversed_amount(2_500),
                ),
            )
            .build()
            .unwrap();
        assert!(partial.contains("<RvsdInstdAmt Ccy=\"EUR\">25.00</RvsdInstdAmt>"));
        assert_validates(&partial, "pain.007.001.09.xsd");
    }

    #[test]
    fn a_group_level_rejection_fixture_is_schema_valid_input() {
        // A fixture that is not itself a real document proves nothing about a
        // parser. This is the shape a bank sends when it refuses a submission
        // outright: a group status, a reason, and no payment-information blocks
        // at all — the case whose reason the parser used to discard.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>STS-1</MsgId><CreDtTm>2026-07-20T09:00:00</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>DD-2026-07-001</OrgnlMsgId>
      <OrgnlMsgNmId>pain.008.001.08</OrgnlMsgNmId>
      <GrpSts>RJCT</GrpSts>
      <StsRsnInf>
        <Rsn><Cd>DUPL</Cd></Rsn>
        <AddtlInf>MsgId already received</AddtlInf>
      </StsRsnInf>
    </OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        assert_validates(xml, "pain.002.001.10.xsd");

        let doc = sepa::parse_pain002(xml).unwrap();
        assert!(!doc.is_fully_accepted());
        assert_eq!(doc.reason_codes().len(), 1);
        assert_eq!(doc.group_additional_info, ["MsgId already received"]);
    }

    #[test]
    fn the_pain002_fixtures_are_schema_valid_input() {
        // The parser is only as good as what it is tested against. This pins
        // that the Verification of Payee shape the unit tests parse is a real
        // pain.002.001.10 document and not something invented.
        let vop = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
  <CstmrPmtStsRpt>
    <GrpHdr>
      <MsgId>B78567267384</MsgId>
      <CreDtTm>2025-11-10T09:31:30Z</CreDtTm>
      <DbtrAgt><FinInstnId><BICFI>SPUEDE2UXXX</BICFI></FinInstnId></DbtrAgt>
    </GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>K563</OrgnlMsgId>
      <OrgnlMsgNmId>pain.001</OrgnlMsgNmId>
      <OrgnlNbOfTxs>462</OrgnlNbOfTxs>
      <GrpSts>RVCM</GrpSts>
      <StsRsnInf><AddtlInf>legal notice</AddtlInf></StsRsnInf>
      <NbOfTxsPerSts><DtldNbOfTxs>454</DtldNbOfTxs><DtldSts>RCVC</DtldSts></NbOfTxsPerSts>
    </OrgnlGrpInfAndSts>
    <OrgnlPmtInfAndSts>
      <OrgnlPmtInfId>B001</OrgnlPmtInfId>
      <TxInfAndSts>
        <OrgnlEndToEndId>K563-B001-T087</OrgnlEndToEndId>
        <TxSts>RVMC</TxSts>
        <StsRsnInf><AddtlInf>Peter Schmitz</AddtlInf></StsRsnInf>
        <OrgnlTxRef>
          <Cdtr><Pty><Nm>P. Schmitz</Nm></Pty></Cdtr>
          <CdtrAcct><Id><IBAN>DE34500500009876543210</IBAN></Id></CdtrAcct>
        </OrgnlTxRef>
      </TxInfAndSts>
    </OrgnlPmtInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        assert_validates(vop, "pain.002.001.10.xsd");

        // …and it parses into the VoP model rather than falling through to
        // `Other`, which is what would happen if the codes were unknown.
        let doc = super::parse_pain002(vop).unwrap();
        let tx = &doc.payment_info_statuses[0].transactions[0];
        assert_eq!(
            tx.status.as_ref().unwrap().verification(),
            Some(sepa::VerificationOutcome::CloseMatch)
        );
        assert_eq!(tx.original_creditor_name.as_deref(), Some("P. Schmitz"));
    }

    #[test]
    fn the_default_schemas_also_satisfy_the_stricter_dk_subset() {
        // The plain ISO schemas are permissive; the Deutsche Kreditwirtschaft
        // publishes technical validation subsets (GBIC 5) that restrict them to
        // what German banks actually accept — mandatory elements, narrowed
        // enumerations. Passing both is a strictly stronger guarantee than
        // passing ISO alone, and it is what caught `OrgnlTxRef` being mandatory
        // on a reversal.
        assert_validates(
            &sct(CreditTransferSchema::IsoV9),
            "pain.001.001.09_GBIC_5.xsd",
        );
        assert_validates(&sdd(DirectDebitSchema::IsoV8), "pain.008.001.08_GBIC_5.xsd");

        // Structured addresses and a timed instant execution are the two newest
        // shapes, and the DK subset is where they are pinned down.
        let addressed = super::Pain001Builder::new("Acme GmbH", "CT-DK-ADR")
            .add_group(
                super::CreditTransferGroup::new(
                    "Acme GmbH",
                    &super::debtor(),
                    "2026-07-20T11:00:00Z".parse::<sepa::IsoDateTime>().unwrap(),
                )
                .kind(sepa::CreditTransferKind::Instant)
                .debtor_bic("COBADEFFXXX".parse().unwrap())
                .add_entry(
                    super::CreditTransferEntry::new(
                        "Supplier AG",
                        super::creditor(),
                        12_000,
                        "E2E-1",
                    )
                    .with_bic("ABNANL2A".parse().unwrap())
                    .with_creditor_address(
                        sepa::PostalAddress::new("Bonn", "DE")
                            .unwrap()
                            .street("Musterlandstrasse")
                            .building_number("47")
                            .post_code("53113"),
                    ),
                ),
            )
            .build()
            .unwrap();
        assert_validates(&addressed, "pain.001.001.09_GBIC_5.xsd");
        assert_validates(&addressed, "pain.001.001.09.xsd");
    }

    #[test]
    fn structured_rf_remittance_validates() {
        // The ISO 11649 path: RmtInf/Strd/CdtrRefInf with Cd=SCOR and Issr=ISO.
        let rf = sepa::RfReference::generate("539007547034").unwrap();
        assert_eq!(rf.as_str(), "RF18539007547034");

        let xml = super::Pain001Builder::new("Acme GmbH", "CT-RF-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), date("2026-07-20"))
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
    fn a_proprietary_structured_reference_validates() {
        // The national-scheme branch: `Cd = SCOR` with a scheme-supplied `Issr`
        // instead of ISO's. It is the one remittance shape whose `Issr` is
        // caller text, so it is the one that has to be schema-checked as well
        // as charset-checked.
        let xml = super::Pain001Builder::new("Acme GmbH", "CT-PROP")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), date("2026-07-20"))
                    .add_entry(
                        super::CreditTransferEntry::new("Payee", super::creditor(), 5_000, "E2E-1")
                            .with_remittance(sepa::RemittanceInfo::Proprietary {
                                reference: "090933755493".to_owned(),
                                issuer: Some("BBA".to_owned()),
                            }),
                    ),
            )
            .build()
            .unwrap();
        assert!(xml.contains("<Issr>BBA</Issr>"), "{xml}");
        assert!(!xml.contains("<Issr>ISO</Issr>"), "ISO is reserved for RF");
        assert_validates(&xml, &schema_file("pain.001.001.09"));
        assert_validates(&xml, "pain.001.001.09_GBIC_5.xsd");
    }

    #[test]
    fn structured_remittance_validates_for_direct_debit() {
        let rf = sepa::RfReference::generate("INV20260042").unwrap();
        let xml = super::Pain008Builder::new("Stadtwerke GmbH", "DD-RF-001")
            .add_group(
                super::DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &super::debtor(),
                    &super::creditor_id(),
                    date("2026-07-20"),
                )
                .add_entry(
                    super::DirectDebitEntry::new(
                        "MND-1",
                        date("2024-06-01"),
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
        let xml = super::Pain001Builder::new("Acme GmbH", "CT-ULT-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), date("2026-07-20"))
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
        let xml = super::Pain008Builder::new("Stadtwerke GmbH", "DD-AMD-001")
            .add_group(
                super::DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &super::debtor(),
                    &super::creditor_id(),
                    date("2026-07-20"),
                )
                .add_entry(
                    super::DirectDebitEntry::new(
                        "MND-1",
                        date("2024-06-01"),
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
    fn creditor_id_amendment_validates_in_every_schema_version() {
        let build = |schema| {
            let old = sepa::validate_creditor_id("DE98ZZZ09999999999").unwrap();
            super::Pain008Builder::new("Stadtwerke GmbH", "DD-CI-CHG")
                .schema(schema)
                .add_group(
                    super::DirectDebitGroup::new(
                        "Stadtwerke GmbH",
                        &super::debtor(),
                        &super::creditor_id(),
                        date("2026-07-20"),
                    )
                    .add_entry(
                        super::DirectDebitEntry::new(
                            "MND-1",
                            date("2024-06-01"),
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

        for schema in DirectDebitSchema::ALL {
            let xml = build(*schema);
            assert!(xml.contains("<OrgnlCdtrSchmeId>"), "{schema}");
            assert!(xml.contains("<Prtry>SEPA</Prtry>"), "{schema}");
            // A creditor-identifier change is not an account change.
            assert!(!xml.contains("SMNDA"), "{schema}");
            assert_validates(&xml, &schema_file(schema.message_id()));
        }
    }

    #[test]
    fn sct_instant_validates_under_the_default_schema() {
        // Regression: SCT Inst uses pain.001.001.09, whose ReqdExctnDt is a
        // DateAndDateTime2Choice. A bare date there failed schema validation.
        let xml = super::Pain001Builder::new("Acme GmbH", "CT-INST-001")
            .add_group(
                super::CreditTransferGroup::new("Acme GmbH", &super::debtor(), date("2026-07-20"))
                    .kind(sepa::pain001::CreditTransferKind::Instant)
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
    fn every_camt055_cancellation_scope_validates() {
        // The three scopes are structurally different documents, and the one
        // that is easiest to get wrong — a group block with no
        // `OrgnlGrpInfAndCxl` beside it — is the one that has to carry
        // `OrgnlGrpInf` to name the file at all.
        for xml in super::camt055_fixtures() {
            assert_validates(&xml, "camt.055.001.05.xsd");
        }
    }

    #[test]
    fn the_camt_parser_fixtures_are_schema_valid_input() {
        // Nothing here generates camt.05x or camt.029, so these schemas gate
        // the *fixtures*: a hand-built document no schema has seen proves only
        // that the parser agrees with whoever wrote it. Same argument as the
        // pain.002 fixtures, which is where it was made first.
        assert_validates(super::CAMT053_BATCH, "camt.053.001.08.xsd");
        for xml in super::camt029_fixtures() {
            assert_validates(&xml, "camt.029.001.06.xsd");
        }
    }

    /// Each newly vendored schema is a real gate, not a permissive stub.
    ///
    /// `tests/fixtures.rs` proves they *accept* the parser's fixtures; a
    /// schema stripped of its restrictions would too. Each must also
    /// **reject** a document breaking a rule only it enforces. That pair is
    /// the whole bound for `pain.002.003.03`, which is single-sourced.
    #[test]
    fn each_vendored_schema_rejects_what_only_it_forbids() {
        // The DK V2.7 status report is reject-only: `GroupStatus3Code` has the
        // single value `RJCT`. An acceptance is not a document this variant
        // can carry, which is a scheme fact no ISO schema states.
        let dk_accepted = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-14T10:20:30</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts>
      <OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.008.003.02</OrgnlMsgNmId>
      <GrpSts>ACTC</GrpSts>
    </OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        assert!(
            !matches!(xmllint(dk_accepted, "pain.002.003.03.xsd"), Some(Ok(()))),
            "pain.002.003.03 must refuse ACTC — it is a reject-only report"
        );

        // camt.053.001.06 makes `Stmt/CreDtTm` mandatory where .08 does not.
        let camt_no_credttm = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.06">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-14T23:59:00</CreDtTm></GrpHdr>
    <Stmt><Id>S</Id>
      <Acct><Id><IBAN>DE89370400440532013000</IBAN></Id></Acct>
      <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy="EUR">0.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-07-14</Dt></Dt></Bal>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;
        assert!(
            !matches!(
                xmllint(camt_no_credttm, "camt.053.001.06.xsd"),
                Some(Ok(()))
            ),
            "camt.053.001.06 must require Stmt/CreDtTm"
        );

        // pain.002.001.03 is the unrestricted ISO original, so the rule to
        // break is a structural one: `OrgnlMsgNmId` is mandatory.
        let iso_missing = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.03">
  <CstmrPmtStsRpt>
    <GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-14T10:20:30</CreDtTm></GrpHdr>
    <OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><GrpSts>ACTC</GrpSts></OrgnlGrpInfAndSts>
  </CstmrPmtStsRpt>
</Document>"#;
        assert!(
            !matches!(xmllint(iso_missing, "pain.002.001.03.xsd"), Some(Ok(()))),
            "pain.002.001.03 must require OrgnlMsgNmId"
        );
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
    let sct_xml = Pain001Builder::new("Acme GmbH", "CT-NP")
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20"))
                .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
        )
        .build()
        .unwrap();
    let sdd_xml = Pain008Builder::new("Stadtwerke GmbH", "DD-NP")
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke GmbH",
                &debtor(),
                &creditor_id(),
                date("2026-07-20"),
            )
            .add_entry(DirectDebitEntry::new(
                "MND-1",
                date("2024-06-01"),
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

    let sct_xml = Pain001Builder::new("Acme GmbH", &msg_id)
        .add_group(
            CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20"))
                .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
        )
        .build()
        .unwrap();

    let sdd_xml = Pain008Builder::new("Stadtwerke GmbH", &msg_id)
        .add_group(
            DirectDebitGroup::new(
                "Stadtwerke GmbH",
                &debtor(),
                &creditor_id(),
                date("2026-07-20"),
            )
            .add_entry(DirectDebitEntry::new(
                "MND-1",
                date("2024-06-01"),
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
        Pain001Builder::new("Acme GmbH", "SHORT")
            .add_group(
                CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20"))
                    .payment_info_id("P".repeat(36))
                    .add_entry(CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1")),
            )
            .build()
            .unwrap_err()
            .kind,
        ValidationError::TooLong {
            field: "PmtInf/PmtInfId",
            ..
        }
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

/// A pain.008 exercising every text-carrying element the writer can emit.
///
/// Shared by the character-set walk and the `Max*Text` walk, so "every element"
/// means the same thing to both and only has to be extended in one place.
fn maximal_direct_debit() -> String {
    let ugly = "Jörg Groß & Söhne — Ψυχή";
    Pain008Builder::new(ugly, "DD-MAX")
        .add_group(
            DirectDebitGroup::new(ugly, &debtor(), &creditor_id(), date("2026-07-20"))
                .creditor_bic("COBADEFFXXX".parse().unwrap())
                .batch_booking(true)
                .category_purpose(sepa::CategoryPurpose::Supp)
                .creditor_address(
                    sepa::PostalAddress::new("Köln", "DE")
                        .unwrap()
                        .department("Buchhaltung")
                        .sub_department("Kreditoren")
                        .street("Große Straße")
                        .building_number("77a")
                        .post_code("50667")
                        .country_subdivision("NRW")
                        .line("2. Obergeschoß"),
                )
                .ultimate_creditor(
                    sepa::Party::new("Fürst & Co").with_organisation_id("CUST-1", Some("Kürzel")),
                )
                .add_entry(
                    DirectDebitEntry::new(
                        "MND-1",
                        date("2024-06-01"),
                        ugly,
                        creditor(),
                        100,
                        "E2E-1",
                    )
                    .with_bic("ABNANL2A".parse().unwrap())
                    .with_description("Abschlag für Straße 1 — 100% fällig")
                    .with_purpose(sepa::Purpose::Other("PHON".to_owned()))
                    .with_ultimate_debtor(
                        sepa::Party::new("Jörg Groß").with_private_id("M-9", Some("Mitglied")),
                    )
                    .with_debtor_address(
                        sepa::PostalAddress::new("Zürich", "CH")
                            .unwrap()
                            .line("Bürogebäude"),
                    )
                    .with_amendment(
                        sepa::pain008::MandateAmendment::mandate_id_changed("OLD-1")
                            .with_original_creditor_name("Vörher GmbH"),
                    ),
                )
                .add_entry(
                    DirectDebitEntry::new(
                        "MND-2",
                        date("2024-06-01"),
                        ugly,
                        creditor(),
                        200,
                        "E2E-2",
                    )
                    .with_remittance(sepa::RemittanceInfo::Proprietary {
                        reference: "REF-42".to_owned(),
                        issuer: Some("Bräuner".to_owned()),
                    }),
                ),
        )
        .build()
        .unwrap()
}

/// A pain.001 exercising every text-carrying element the writer can emit.
fn maximal_credit_transfer() -> String {
    let ugly = "Jörg Groß & Söhne — Ψυχή";
    Pain001Builder::new(ugly, "CT-MAX")
        .add_group(
            CreditTransferGroup::new(ugly, &debtor(), date("2026-07-20"))
                .debtor_bic("COBADEFFXXX".parse().unwrap())
                .batch_booking(false)
                .category_purpose(sepa::CategoryPurpose::Sala)
                .debtor_address(
                    sepa::PostalAddress::new("Köln", "DE")
                        .unwrap()
                        .street("Größe"),
                )
                .ultimate_debtor(sepa::Party::new("Mütterlich GmbH"))
                .add_entry(
                    CreditTransferEntry::new(ugly, creditor(), 100, "E2E-1")
                        .with_bic("ABNANL2A".parse().unwrap())
                        .with_creditor_address(
                            sepa::PostalAddress::new("Zürich", "CH")
                                .unwrap()
                                .line("Bürogebäude"),
                        )
                        .with_ultimate_creditor(sepa::Party::new("Endbegünstigter"))
                        .with_purpose(sepa::Purpose::Other("RENT".to_owned()))
                        .with_reference(sepa::RfReference::generate("2026-0042").unwrap()),
                ),
        )
        .build()
        .unwrap()
}

/// A pain.007 exercising every text-carrying element the writer can emit.
/// A camt.055 carrying every text-bearing element the writer can emit.
///
/// The two document-wide walks below share these fixtures, so a writer that
/// gains an element gains coverage in both at once — which is the whole reason
/// they are shared rather than written twice.
fn maximal_cancellation() -> String {
    use sepa::{
        Camt055Builder, CancellationEntry, CancellationGroup, CancellationReason, OriginalMessage,
        Party, validate_bic,
    };

    let ugly = "Jörg Groß & Söhne";
    Camt055Builder::new(
        "CXL-MAX",
        Party::new(ugly).with_organisation_id("CUST-4711", Some("Größe")),
        validate_bic("COBADEFFXXX").unwrap(),
        OriginalMessage::new("DD-MAX", "pain.008.001.08")
            .created_at("2026-07-15T09:00:00".parse().unwrap())
            .totals(2, 15_000),
    )
    .created_at("2026-07-15T11:30:00".parse().unwrap())
    .case_id("CASE-MAX")
    .add_group(
        CancellationGroup::new("PMT-MAX")
            .payment_cancellation_id("PC-MAX")
            .add_entry(
                CancellationEntry::new("E2E-1", CancellationReason::Dupl)
                    .cancellation_id("CX-1")
                    .original_instruction_id("INSTR-1")
                    .original_amount(7_500)
                    .additional_info("Doppelte Einreichung für Jörg Groß"),
            )
            .add_entry(
                CancellationEntry::new("E2E-2", "XY99".parse().unwrap())
                    .original_amount(7_500)
                    .additional_info("Größe & Söhne"),
            ),
    )
    .build()
    .unwrap()
}

fn maximal_reversal() -> String {
    let ugly = "Jörg Groß & Söhne";
    let group = DirectDebitGroup::new(ugly, &debtor(), &creditor_id(), date("2026-07-20"))
        .creditor_bic("COBADEFFXXX".parse().unwrap());
    let entry = DirectDebitEntry::new(
        "MND-42",
        date("2024-06-01"),
        ugly,
        creditor(),
        7_500,
        "E2E-1",
    )
    .with_bic("ABNANL2A".parse().unwrap());
    sepa::Pain007Builder::new(ugly, "DD-MAX", "RVSL-MAX")
        .add_group(
            sepa::ReversalGroup::new("DD-MAX").add_entry(sepa::ReversalEntry::reverse(
                &group,
                &entry,
                sepa::ReversalReason::Ms02,
            )),
        )
        .build()
        .unwrap()
}

#[test]
fn every_emitted_text_value_is_in_the_sepa_character_set() {
    // Every text node, not a hand-listed set of tags. The list is the bug:
    // `RmtInf/Strd/CdtrRefInf/Tp/Issr` went neither validated nor
    // transliterated because nobody thought to add the tag, and a per-tag
    // assertion cannot see that. Walking the document means the next element
    // added to a writer is covered the day it is added.
    for xml in [
        &maximal_direct_debit(),
        &maximal_credit_transfer(),
        &maximal_reversal(),
        &maximal_cancellation(),
    ] {
        for (_, element, text) in text_nodes_with_path(xml) {
            assert!(
                sepa::is_sepa_text(&text),
                "{element} emitted {text:?}, which is not in the SEPA character set"
            );
        }
    }
}

#[test]
fn invalid_batches_are_rejected_before_any_xml_is_produced() {
    let group = || CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20"));
    let base = || Pain001Builder::new("Acme GmbH", "CT-1");
    let entry = |ct| CreditTransferEntry::new("Payee", creditor(), ct, "E2E-1");

    assert_eq!(
        base().build(),
        Err(BuildError::message(ValidationError::EmptyBatch))
    );
    assert_eq!(
        base().add_group(group().add_entry(entry(0))).build(),
        Err(BuildError::transaction(
            0,
            0,
            ValidationError::AmountOutOfRange {
                field: "CdtTrfTxInf/Amt/InstdAmt",
                amount_ct: 0,
            }
        ))
    );
    assert!(matches!(
        base()
            .add_group(group().add_entry(entry(100_000_000_000)))
            .build()
            .unwrap_err()
            .kind,
        ValidationError::AmountOutOfRange { .. }
    ));

    // A date that is not a real calendar day never gets as far as a batch: it
    // is rejected where the `IsoDate` is constructed.
    assert!("2026-02-30".parse::<sepa::IsoDate>().is_err());
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
    assert_eq!(tx.original_end_to_end_id.as_deref(), Some("INV-2026-001"));
    assert_eq!(tx.original_amount_ct, Some(12_000));
    // Entities and numeric character references are decoded.
    assert_eq!(tx.original_creditor_name.as_deref(), Some("Blümel & Söhne"));
    assert_eq!(
        tx.original_creditor_iban.as_deref(),
        Some("NL91ABNA0417164300")
    );
}

/// The camt fixtures the parser tests read, so the schema gate and the parser
/// see the same bytes.
///
/// A hand-built fixture no schema has seen proves only that the parser agrees
/// with whoever wrote it — which is the argument that put the pain.002 example
/// through `pain.002.001.10.xsd`, and it applies identically here.
const CAMT053_BATCH: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
        <BkTxCd><Domn><Cd>PMNT</Cd><Fmly><Cd>RDDT</Cd><SubFmlyCd>PMDD</SubFmlyCd></Fmly></Domn></BkTxCd>
        <NtryDtls>
          <TxDtls>
            <Refs><EndToEndId>E2E-1</EndToEndId><MndtId>MND-1</MndtId></Refs>
            <Amt Ccy="EUR">100.00</Amt>
            <RltdPties><Dbtr><Pty><Nm>Kunde Eins</Nm></Pty></Dbtr></RltdPties>
          </TxDtls>
          <TxDtls>
            <Refs><EndToEndId>E2E-2</EndToEndId><MndtId>MND-2</MndtId></Refs>
            <Amt Ccy="EUR">75.00</Amt>
            <RltdPties><Dbtr><Pty><Nm>Kunde Zwei</Nm></Pty></Dbtr></RltdPties>
          </TxDtls>
          <TxDtls>
            <Refs><EndToEndId>E2E-3</EndToEndId><MndtId>MND-3</MndtId></Refs>
            <Amt Ccy="EUR">50.00</Amt>
            <RtrInf><Rsn><Cd>MD01</Cd></Rsn></RtrInf>
          </TxDtls>
        </NtryDtls>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

#[test]
fn camt053_batch_booking_exposes_every_transaction() {
    // A batch-booked direct debit collection: one aggregate entry, three
    // underlying transactions. Reconciliation needs all three.
    let xml = CAMT053_BATCH;

    let doc = parse_camt053(xml).unwrap();
    let stmt = &doc.statements[0];
    assert_eq!(stmt.account.iban.as_deref(), Some("DE89370400440532013000"));
    assert_eq!(stmt.closing_balance().unwrap().signed_ct(), Some(122_500));

    let entry = &stmt.entries[0];
    assert_eq!(entry.amount.ct, Some(22_500));
    assert_eq!(entry.amount.currency, Some("EUR".to_owned()));
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
    let sum: i64 = entry.details.iter().filter_map(|d| d.amount.ct).sum();
    assert_eq!(Some(sum), entry.amount.ct);
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
        assert_eq!(e.signed_ct(), Some(15_542));
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

    let doc = parse_camt053(xml).unwrap();
    let entries = &doc.statements[0].entries;

    // The booking is REPORTED, not dropped. A dropped entry is the one kind
    // of parse failure an importer cannot detect: a missing booking looks
    // exactly like a booking that never happened.
    assert_eq!(
        entries.len(),
        1,
        "an unreadable amount must not delete a booking"
    );
    let entry = &entries[0];
    assert_eq!(entry.amount.ct, None, "the amount is not invented");
    assert_eq!(entry.signed_ct(), None, "and neither is a ledger figure");
    assert_eq!(
        entry.amount.amount_raw.as_deref(),
        Some("1.\u{20ac}5"),
        "what the bank actually sent survives for the operator to look at"
    );
    // And the statement's net movement refuses to answer rather than
    // understating itself by the row it could not read.
    assert_eq!(doc.statements[0].net_movement_ct(), None);
}

#[test]
fn an_unreadable_credit_debit_indicator_is_never_taken_for_a_credit() {
    // Regression, and the worst defect this crate has had. `CdtDbtInd` was
    // parsed with `.unwrap_or(Credit)`, so a debit whose indicator a bank
    // mistyped — or spelled in a case this parser did not fold — was reported
    // as a credit of the same magnitude. In a ledger that is a two-for-one
    // error: EUR 1000 removed from the wrong side and added to the other.
    let doc_with = |ind: &str| {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S1</Id>
    <Ntry><Amt Ccy="EUR">1000.00</Amt>{ind}</Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#
        );
        parse_camt053(&xml).unwrap()
    };

    // The two codes that exist still work, in both cases.
    assert_eq!(
        doc_with("<CdtDbtInd>CRDT</CdtDbtInd>").statements[0].entries[0].signed_ct(),
        Some(100_000)
    );
    assert_eq!(
        doc_with("<CdtDbtInd>DBIT</CdtDbtInd>").statements[0].entries[0].signed_ct(),
        Some(-100_000)
    );

    // Everything else refuses to answer, and keeps what arrived.
    for bad in ["<CdtDbtInd>DBTI</CdtDbtInd>", "<CdtDbtInd></CdtDbtInd>", ""] {
        let doc = doc_with(bad);
        let entry = &doc.statements[0].entries[0];
        assert_eq!(
            entry.amount.direction, None,
            "{bad:?} must not resolve to a direction"
        );
        assert_eq!(
            entry.signed_ct(),
            None,
            "{bad:?} must not produce a ledger figure"
        );
        assert_eq!(
            entry.amount.ct,
            Some(100_000),
            "{bad:?} — the magnitude is still known; only the direction is not"
        );
    }
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

fn assert_writer_boundary(
    buffered: Result<String, BuildError>,
    streamed: Result<(), sepa::WriteError>,
    sink: &[u8],
    invalid_location: Option<sepa::Location>,
    expected_text: &str,
) {
    if let Some(location) = invalid_location {
        let error = buffered.unwrap_err();
        let sepa::WriteError::Validation(stream_error) = streamed.unwrap_err() else {
            panic!("invalid input must fail validation before I/O");
        };
        assert_eq!(error.location, location);
        assert_eq!(stream_error.location, location);
        assert_eq!(error.kind.to_string(), stream_error.kind.to_string());
        assert_eq!(sink, b"existing");
    } else {
        let xml = buffered.unwrap();
        streamed.unwrap();
        assert!(xml.contains(expected_text), "missing {expected_text:?}");
        assert_eq!(&sink[..8], b"existing");
        assert_eq!(&sink[8..], xml.as_bytes());
    }
}

#[test]
#[cfg(feature = "serde")]
fn restored_parties_are_checked_at_every_payment_position() {
    let cases = [
        (
            serde_json::json!({"name": "End party"}),
            true,
            "<Nm>End party</Nm>",
        ),
        (
            serde_json::json!({"identifier": {"kind": "Private", "id": "CUSTOMER-1", "scheme_name": "CUSTOMER"}}),
            true,
            "<Id>CUSTOMER-1</Id>",
        ),
        (serde_json::json!({}), false, ""),
        (serde_json::json!({"name": ""}), false, ""),
        (serde_json::json!({"name": "A".repeat(71)}), false, ""),
        (
            serde_json::json!({"identifier": {"kind": "Organisation", "id": "A\u{0}B"}}),
            false,
            "",
        ),
        (
            serde_json::json!({"identifier": {"kind": "Organisation", "id": "A//B"}}),
            false,
            "",
        ),
        (
            serde_json::json!({"identifier": {"kind": "Organisation", "id": "CUSTOMER-1", "scheme_name": "A".repeat(36)}}),
            false,
            "",
        ),
    ];
    for (stored, valid, expected) in cases {
        let party: sepa::Party = serde_json::from_value(stored).unwrap();
        for policy in [sepa::CharsetPolicy::Strict, sepa::CharsetPolicy::default()] {
            for position in 0..3 {
                let location = (!valid).then_some(if position == 0 {
                    sepa::Location::group(0)
                } else {
                    sepa::Location::transaction(0, 0)
                });
                let mut ct_entry = CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1");
                let mut ct_group =
                    CreditTransferGroup::new("Debtor", &debtor(), date("2026-07-20"));
                let mut dd_entry = DirectDebitEntry::new(
                    "MND-1",
                    date("2024-06-01"),
                    "Debtor",
                    debtor(),
                    100,
                    "E2E-1",
                );
                let mut dd_group = DirectDebitGroup::new(
                    "Creditor",
                    &creditor(),
                    &creditor_id(),
                    date("2026-07-20"),
                );
                match position {
                    0 => {
                        ct_group = ct_group.ultimate_debtor(party.clone());
                        dd_group = dd_group.ultimate_creditor(party.clone());
                    }
                    1 => {
                        ct_entry = ct_entry.with_ultimate_debtor(party.clone());
                        dd_entry = dd_entry.with_ultimate_debtor(party.clone());
                    }
                    _ => {
                        ct_entry = ct_entry.with_ultimate_creditor(party.clone());
                        dd_entry = dd_entry.with_ultimate_creditor(party.clone());
                    }
                }
                let ct = Pain001Builder::new("Debtor", "CT-PARTY")
                    .charset(policy)
                    .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                    .add_group(ct_group.add_entry(ct_entry));
                let dd = Pain008Builder::new("Creditor", "DD-PARTY")
                    .charset(policy)
                    .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                    .add_group(dd_group.add_entry(dd_entry));
                let mut sink = b"existing".to_vec();
                assert_writer_boundary(
                    ct.build(),
                    ct.write_to(&mut sink),
                    &sink,
                    location,
                    expected,
                );
                let mut sink = b"existing".to_vec();
                assert_writer_boundary(
                    dd.build(),
                    dd.write_to(&mut sink),
                    &sink,
                    location,
                    expected,
                );
            }
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn purpose_codes_are_checked_before_buffered_or_streamed_output() {
    fn check(
        buffered: Result<String, BuildError>,
        streamed: Result<(), sepa::WriteError>,
        sink: &[u8],
        code: &str,
        invalid_location: Option<sepa::Location>,
    ) {
        assert_writer_boundary(
            buffered,
            streamed,
            sink,
            invalid_location,
            &format!("<Cd>{code}</Cd>"),
        );
    }

    #[cfg(feature = "serde")]
    let routes = [false, true].as_slice();
    #[cfg(not(feature = "serde"))]
    let routes = [false].as_slice();
    for &restored in routes {
        for (code, valid) in [
            (" SALA ", false),
            ("sala", false),
            (" A ", false),
            ("", false),
            ("ABCDE", false),
            ("A\0", false),
            ("<X>", false),
            ("A", true),
            ("A1", true),
            ("ZZZZ", true),
            ("SALA", true),
        ] {
            let purpose = sepa::Purpose::Other(code.to_owned());
            let category = sepa::CategoryPurpose::Other(code.to_owned());
            #[cfg(feature = "serde")]
            let (purpose, category) = if restored {
                (
                    serde_json::from_value(serde_json::to_value(purpose).unwrap()).unwrap(),
                    serde_json::from_value(serde_json::to_value(category).unwrap()).unwrap(),
                )
            } else {
                (purpose, category)
            };
            #[cfg(not(feature = "serde"))]
            let _ = restored;
            for group_code in [false, true] {
                let location = (!valid).then_some(if group_code {
                    sepa::Location::group(0)
                } else {
                    sepa::Location::transaction(0, 0)
                });
                let mut ct_entry = CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1");
                let mut ct_group =
                    CreditTransferGroup::new("Debtor", &debtor(), date("2026-07-20"));
                let mut dd_entry = DirectDebitEntry::new(
                    "MND-1",
                    date("2024-06-01"),
                    "Debtor",
                    debtor(),
                    100,
                    "E2E-1",
                );
                let mut dd_group = DirectDebitGroup::new(
                    "Creditor",
                    &creditor(),
                    &creditor_id(),
                    date("2026-07-20"),
                );
                if group_code {
                    ct_group = ct_group.category_purpose(category.clone());
                    dd_group = dd_group.category_purpose(category.clone());
                } else {
                    ct_entry = ct_entry.with_purpose(purpose.clone());
                    dd_entry = dd_entry.with_purpose(purpose.clone());
                }
                let ct = Pain001Builder::new("Debtor", "CT-CODE")
                    .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                    .add_group(ct_group.add_entry(ct_entry));
                let dd = Pain008Builder::new("Creditor", "DD-CODE")
                    .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                    .add_group(dd_group.add_entry(dd_entry));
                let mut sink = b"existing".to_vec();
                check(ct.build(), ct.write_to(&mut sink), &sink, code, location);
                let mut sink = b"existing".to_vec();
                check(dd.build(), dd.write_to(&mut sink), &sink, code, location);
            }
        }
    }
}

#[test]
#[cfg(feature = "serde")]
fn restored_recall_parties_are_checked_before_output() {
    for (stored, valid) in [
        (serde_json::json!({"name": "End party"}), true),
        (
            serde_json::json!({"identifier": {"kind": "Organisation", "id": "CUSTOMER-1"}}),
            true,
        ),
        (serde_json::json!({}), false),
        (serde_json::json!({"name": "A".repeat(71)}), false),
        (
            serde_json::json!({"identifier": {"kind": "Private", "id": "A//B"}}),
            false,
        ),
    ] {
        let party: sepa::Party = serde_json::from_value(stored).unwrap();
        for policy in [sepa::CharsetPolicy::Strict, sepa::CharsetPolicy::default()] {
            for assigner_position in [false, true] {
                let (requester, recipient) = if assigner_position {
                    (party.clone(), sepa::Party::new("Bank"))
                } else {
                    (sepa::Party::new("Customer"), party.clone())
                };
                let builder = sepa::Camt055Builder::new(
                    "RECALL-1",
                    requester,
                    recipient,
                    sepa::OriginalMessage::new("ORIGINAL-1", "pain.008.001.08"),
                )
                .case_id("CASE-1")
                .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                .charset(policy)
                .cancel_whole_message(sepa::CancellationReason::Cust);
                let mut sink = b"existing".to_vec();
                assert_writer_boundary(
                    builder.build(),
                    builder.write_to(&mut sink),
                    &sink,
                    (!valid).then_some(sepa::Location::message()),
                    "<OrgnlMsgId>ORIGINAL-1</OrgnlMsgId>",
                );
            }
        }
    }
}

#[test]
#[cfg(feature = "serde")]
#[allow(clippy::too_many_lines)]
fn restored_remittance_is_checked_by_both_payment_writers() {
    for (remittance, valid, expected) in [
        (
            sepa::RemittanceInfo::unstructured("Invoice 1"),
            true,
            "<Ustrd>Invoice 1</Ustrd>",
        ),
        (
            sepa::RemittanceInfo::structured("RF18539007547034".parse().unwrap()),
            true,
            "<Ref>RF18539007547034</Ref>",
        ),
        (
            sepa::RemittanceInfo::Proprietary {
                reference: "INV-1".into(),
                issuer: Some("SCHEME".into()),
            },
            true,
            "<Ref>INV-1</Ref>",
        ),
        (sepa::RemittanceInfo::unstructured(""), false, ""),
        (
            sepa::RemittanceInfo::unstructured("A".repeat(141)),
            false,
            "",
        ),
        (
            sepa::RemittanceInfo::Proprietary {
                reference: String::new(),
                issuer: None,
            },
            false,
            "",
        ),
        (
            sepa::RemittanceInfo::Proprietary {
                reference: "A".repeat(36),
                issuer: None,
            },
            false,
            "",
        ),
        (
            sepa::RemittanceInfo::Proprietary {
                reference: "INV-1".into(),
                issuer: Some("A".repeat(36)),
            },
            false,
            "",
        ),
    ] {
        let restored: sepa::RemittanceInfo =
            serde_json::from_value(serde_json::to_value(remittance).unwrap()).unwrap();
        for policy in [sepa::CharsetPolicy::Strict, sepa::CharsetPolicy::default()] {
            let mut ct_entry = CreditTransferEntry::new("Payee", creditor(), 100, "E2E-1");
            ct_entry.remittance = Some(restored.clone());
            let ct = Pain001Builder::new("Debtor", "CT-RMT")
                .charset(policy)
                .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                .add_group(
                    CreditTransferGroup::new("Debtor", &debtor(), date("2026-07-20"))
                        .add_entry(ct_entry),
                );
            let mut dd_entry = DirectDebitEntry::new(
                "MND-1",
                date("2024-06-01"),
                "Debtor",
                debtor(),
                100,
                "E2E-1",
            );
            dd_entry.remittance = Some(restored.clone());
            let dd = Pain008Builder::new("Creditor", "DD-RMT")
                .charset(policy)
                .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                .add_group(
                    DirectDebitGroup::new(
                        "Creditor",
                        &creditor(),
                        &creditor_id(),
                        date("2026-07-20"),
                    )
                    .add_entry(dd_entry),
                );
            let location = (!valid).then_some(sepa::Location::transaction(0, 0));
            let mut sink = b"existing".to_vec();
            assert_writer_boundary(
                ct.build(),
                ct.write_to(&mut sink),
                &sink,
                location,
                expected,
            );
            let mut sink = b"existing".to_vec();
            assert_writer_boundary(
                dd.build(),
                dd.write_to(&mut sink),
                &sink,
                location,
                expected,
            );
        }
    }
}

#[test]
#[cfg(feature = "serde")]
#[allow(clippy::too_many_lines)]
fn restored_original_records_and_amendments_are_checked_before_output() {
    for (field, value, valid) in [
        ("mandate_ref", serde_json::json!("MND-1"), true),
        ("mandate_ref", serde_json::json!(""), false),
        ("mandate_ref", serde_json::json!("A//B"), false),
        (
            "debtor",
            serde_json::json!(["A".repeat(71), "DE89370400440532013000"]),
            false,
        ),
        (
            "creditor",
            serde_json::json!(["", "NL91ABNA0417164300"]),
            false,
        ),
    ] {
        let mut stored =
            serde_json::to_value(sepa::OriginalCollection::new("MND-1", date("2024-06-01")))
                .unwrap();
        stored[field] = value;
        let original: sepa::OriginalCollection = serde_json::from_value(stored).unwrap();
        let builder =
            sepa::Pain007Builder::new("Creditor", "ORIGINAL-1", "REVERSAL-1")
                .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                .add_group(sepa::ReversalGroup::new("GROUP-1").add_entry(
                    sepa::ReversalEntry::new("E2E-1", 100, sepa::ReversalReason::Cust, original),
                ));
        let mut sink = b"existing".to_vec();
        assert_writer_boundary(
            builder.build(),
            builder.write_to(&mut sink),
            &sink,
            (!valid).then_some(sepa::Location::transaction(0, 0)),
            "<MndtId>MND-1</MndtId>",
        );
    }
    for (stored, valid) in [
        (serde_json::json!({"same_mandate_new_account": true}), true),
        (serde_json::json!({"original_mandate_id": "MND-OLD"}), true),
        (serde_json::json!({}), false),
        (serde_json::json!({"original_mandate_id": "A//B"}), false),
        (
            serde_json::json!({"original_creditor_name": "A".repeat(71)}),
            false,
        ),
        (
            serde_json::json!({"same_mandate_new_account": true, "original_debtor_iban": "DE89370400440532013000"}),
            false,
        ),
    ] {
        let mut complete = serde_json::to_value(sepa::MandateAmendment::default()).unwrap();
        complete
            .as_object_mut()
            .unwrap()
            .extend(stored.as_object().unwrap().clone());
        let amendment: sepa::MandateAmendment = serde_json::from_value(complete).unwrap();
        let builder = Pain008Builder::new("Creditor", "DD-AMENDMENT")
            .created_at("2026-07-19T12:00:00Z".parse().unwrap())
            .add_group(
                DirectDebitGroup::new("Creditor", &creditor(), &creditor_id(), date("2026-07-20"))
                    .add_entry(
                        DirectDebitEntry::new(
                            "MND-1",
                            date("2024-06-01"),
                            "Debtor",
                            debtor(),
                            100,
                            "E2E-1",
                        )
                        .with_amendment(amendment),
                    ),
            );
        let mut sink = b"existing".to_vec();
        assert_writer_boundary(
            builder.build(),
            builder.write_to(&mut sink),
            &sink,
            (!valid).then_some(sepa::Location::transaction(0, 0)),
            "<AmdmntInd>true</AmdmntInd>",
        );
    }
    for (field, value, valid) in [
        ("message_id", serde_json::json!("ORIGINAL-1"), true),
        ("message_id", serde_json::json!(""), false),
        ("message_name_id", serde_json::json!("A//B"), false),
    ] {
        let mut stored =
            serde_json::to_value(sepa::OriginalMessage::new("ORIGINAL-1", "pain.008.001.08"))
                .unwrap();
        stored[field] = value;
        let original: sepa::OriginalMessage = serde_json::from_value(stored).unwrap();
        let builder = sepa::Camt055Builder::new("RECALL-1", "Customer", "Bank", original)
            .created_at("2026-07-19T12:00:00Z".parse().unwrap())
            .cancel_whole_message(sepa::CancellationReason::Cust);
        let mut sink = b"existing".to_vec();
        assert_writer_boundary(
            builder.build(),
            builder.write_to(&mut sink),
            &sink,
            (!valid).then_some(sepa::Location::message()),
            "<OrgnlMsgId>ORIGINAL-1</OrgnlMsgId>",
        );
    }
}

#[test]
fn original_recall_totals_obey_the_pinned_schema_bounds() {
    for (count, amount, valid) in [
        (1, 100, true),
        (0, -100, true),
        (999_999_999_999_999, 999_999_999_999_999_999, true),
        (1, 9_223_372_036_854_775_800, true),
        (1_000_000_000_000_000, 100, false),
        (u64::MAX, 100, false),
        (1, i64::MAX, false),
        (1, i64::MIN, false),
    ] {
        let original =
            sepa::OriginalMessage::new("ORIGINAL-1", "pain.008.001.08").totals(count, amount);
        #[cfg(feature = "serde")]
        let originals = [
            original.clone(),
            serde_json::from_value(serde_json::to_value(original).unwrap()).unwrap(),
        ]
        .to_vec();
        #[cfg(not(feature = "serde"))]
        let originals = vec![original];
        for original in originals {
            let builder = sepa::Camt055Builder::new("RECALL-1", "Customer", "Bank", original)
                .created_at("2026-07-19T12:00:00Z".parse().unwrap())
                .cancel_whole_message(sepa::CancellationReason::Cust);
            let mut sink = b"existing".to_vec();
            assert_writer_boundary(
                builder.build(),
                builder.write_to(&mut sink),
                &sink,
                (!valid).then_some(sepa::Location::message()),
                &format!("<NbOfTxs>{count}</NbOfTxs>"),
            );
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_writer_refuses_invalid_reference_positions_before_output() {
    for payload in ["X<Injected>".to_owned(), "A\0B".to_owned(), "A".repeat(36)] {
        for policy in [sepa::CharsetPolicy::Strict, sepa::CharsetPolicy::default()] {
            macro_rules! refuse {
                ($builder:expr, $location:expr) => {{
                    let builder = $builder
                        .charset(policy)
                        .created_at("2026-07-19T12:00:00Z".parse().unwrap());
                    let mut sink = b"existing".to_vec();
                    assert_writer_boundary(
                        builder.build(),
                        builder.write_to(&mut sink),
                        &sink,
                        Some($location),
                        "",
                    );
                }};
            }
            for position in 0..5 {
                let value = |slot| {
                    if position == slot {
                        payload.as_str()
                    } else {
                        "SAFE"
                    }
                };
                let mut ct_entry = CreditTransferEntry::new("Payee", creditor(), 100, value(2))
                    .with_ultimate_creditor(
                        sepa::Party::new("Party").with_organisation_id(value(3), None),
                    );
                ct_entry.remittance = Some(sepa::RemittanceInfo::Proprietary {
                    reference: value(4).into(),
                    issuer: None,
                });
                refuse!(
                    Pain001Builder::new("Debtor", value(0)).add_group(
                        CreditTransferGroup::new("Debtor", &debtor(), date("2026-07-20"))
                            .payment_info_id(value(1))
                            .add_entry(ct_entry)
                    ),
                    match position {
                        0 => sepa::Location::message(),
                        1 => sepa::Location::group(0),
                        _ => sepa::Location::transaction(0, 0),
                    }
                );
                let dd_entry = DirectDebitEntry::new(
                    value(3),
                    date("2024-06-01"),
                    "Debtor",
                    debtor(),
                    100,
                    value(2),
                )
                .with_amendment(sepa::MandateAmendment::mandate_id_changed(value(4)));
                refuse!(
                    Pain008Builder::new("Creditor", value(0)).add_group(
                        DirectDebitGroup::new(
                            "Creditor",
                            &creditor(),
                            &creditor_id(),
                            date("2026-07-20")
                        )
                        .payment_info_id(value(1))
                        .add_entry(dd_entry)
                    ),
                    match position {
                        0 => sepa::Location::message(),
                        1 => sepa::Location::group(0),
                        _ => sepa::Location::transaction(0, 0),
                    }
                );
            }
            for position in 0..7 {
                let value = |slot| {
                    if position == slot {
                        payload.as_str()
                    } else {
                        "SAFE"
                    }
                };
                let reason = if position == 6 {
                    sepa::ReversalReason::Other(payload.clone())
                } else {
                    sepa::ReversalReason::Cust
                };
                refuse!(
                    sepa::Pain007Builder::new("Creditor", value(1), value(0)).add_group(
                        sepa::ReversalGroup::new(value(2))
                            .reversal_payment_info_id(value(3))
                            .add_entry(sepa::ReversalEntry::new(
                                value(4),
                                100,
                                reason,
                                sepa::OriginalCollection::new(value(5), date("2024-06-01"))
                            ))
                    ),
                    match position {
                        0 | 1 => sepa::Location::message(),
                        2 | 3 => sepa::Location::group(0),
                        _ => sepa::Location::transaction(0, 0),
                    }
                );
            }
            for position in 0..12 {
                let value = |slot| {
                    if position == slot {
                        payload.as_str()
                    } else {
                        "SAFE"
                    }
                };
                let reason = if position == 11 {
                    sepa::CancellationReason::Other(payload.clone())
                } else {
                    sepa::CancellationReason::Cust
                };
                let requester = sepa::Party::new("Customer").with_organisation_id(value(4), None);
                let recipient = sepa::Party::new("Bank").with_private_id(value(5), None);
                refuse!(
                    sepa::Camt055Builder::new(
                        value(0),
                        requester,
                        recipient,
                        sepa::OriginalMessage::new(value(2), value(3))
                    )
                    .case_id(value(1))
                    .add_group(
                        sepa::CancellationGroup::new(value(6))
                            .payment_cancellation_id(value(7))
                            .add_entry(
                                sepa::CancellationEntry::new(value(8), reason)
                                    .cancellation_id(value(9))
                                    .original_instruction_id(value(10))
                            )
                    ),
                    match position {
                        0..=5 => sepa::Location::message(),
                        6 | 7 => sepa::Location::group(0),
                        _ => sepa::Location::transaction(0, 0),
                    }
                );
            }
        }
    }
}

#[test]
fn streaming_output_matches_the_in_memory_build() {
    let build = || {
        Pain008Builder::new("Stadtwerke GmbH", "DD-STREAM")
            .created_at("2026-07-19T12:00:00".parse().unwrap())
            .add_group(
                DirectDebitGroup::new(
                    "Stadtwerke GmbH",
                    &debtor(),
                    &creditor_id(),
                    date("2026-07-20"),
                )
                .add_entry(DirectDebitEntry::new(
                    "MND-1",
                    date("2024-06-01"),
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
    let err = Pain008Builder::new("Stadtwerke GmbH", "DD-EMPTY")
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

    let builder = Pain001Builder::new("Acme GmbH", "BULK").add_group(
        CreditTransferGroup::new("Acme GmbH", &debtor(), date("2026-07-20")).add_entries(entries),
    );

    assert_eq!(builder.entry_count(), 10_000);
    assert_eq!(builder.total_ct(), 10_000);

    let xml = builder.build().unwrap();
    assert!(xml.contains("<NbOfTxs>10000</NbOfTxs>"));
    assert!(xml.contains("<CtrlSum>100.00</CtrlSum>"));
}

/// Every `(parent, element, text)` triple in a document this crate produced.
///
/// A deliberately small scanner rather than a parser: the only markup in these
/// documents is the markup the writers emit, and the point is to see the tree
/// the way a *bank's* parser would rather than the way the builder meant it.
fn text_nodes_with_path(xml: &str) -> Vec<(Option<String>, String, String)> {
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        // Text preceding this tag belongs to the element currently open.
        let text = rest[..open].trim();
        if !text.is_empty()
            && let Some(element) = stack.last()
        {
            let parent = stack
                .len()
                .checked_sub(2)
                .and_then(|i| stack.get(i))
                .cloned();
            out.push((
                parent,
                element.clone(),
                text.replace("&amp;", "&")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'"),
            ));
        }
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            assert_eq!(
                stack.pop().as_deref(),
                Some(name),
                "unbalanced element </{name}>"
            );
        } else {
            let name = tag.split([' ', '\t']).next().unwrap_or(tag);
            assert!(!tag.ends_with('/'), "the writers emit no self-closing tags");
            stack.push(name.to_owned());
        }
    }
    assert!(stack.is_empty(), "unclosed elements: {stack:?}");
    out
}

/// Elements whose text the `Max*Text` table deliberately does not bound.
///
/// Either a value bounded by its own type (`IBAN`, `BICFI`), a date, an amount,
/// a boolean, a count, or a fixed enumerated code. An element that is in
/// neither this list nor `max_text_len` is a writer emitting text nobody
/// bounded — which is the defect class `Tp/Issr` belonged to.
const UNBOUNDED_BY_TYPE: &[&str] = &[
    "AmdmntInd",     // boolean
    "BIC",           // ISO 9362, validated by `Bic`
    "BICFI",         // ISO 9362, validated by `Bic`
    "BtchBookg",     // boolean
    "ChrgBr",        // enumerated: SLEV
    "CreDtTm",       // ISO 8601, validated by `IsoDateTime`
    "CtrlSum",       // decimal
    "Ctry",          // ISO 3166-1 alpha-2, validated by `country`
    "Dt",            // ISO 8601, validated by `IsoDate`
    "DtOfSgntr",     // ISO 8601, validated by `IsoDate`
    "DtTm",          // ISO 8601, validated by `IsoDateTime`
    "IBAN",          // ISO 13616, validated by `Iban`
    "InstdAmt",      // decimal
    "NbOfTxs",       // count
    "OrgnlInstdAmt", // decimal
    "PmtMtd",        // enumerated: TRF / DD
    "ReqdColltnDt",  // ISO 8601, validated by `IsoDate`
    "RvsdInstdAmt",  // decimal
    "SeqTp",         // enumerated: FRST / RCUR / FNAL / OOFF
    // camt.055
    "CtrlSum",           // decimal — also listed above for the pain messages
    "GrpCxl",            // boolean
    "OrgnlCreDtTm",      // ISO 8601, validated by `IsoDateTime`
    "OrgnlInstdAmt",     // decimal
    "OrgnlReqdColltnDt", // ISO 8601, validated by `IsoDate`
    "OrgnlReqdExctnDt",  // ISO 8601, validated by `IsoDate`
    "PmtInfCxl",         // boolean
];

#[test]
fn every_emitted_text_value_is_within_its_max_text_bound() {
    // The companion to the character-set walk, and the same argument: a length
    // limit written at each call site is a limit that can be forgotten at the
    // next call site. `validate::max_text_len` is the one table, and this test
    // requires it to explain every element the writers actually emit — so a new
    // element cannot ship without a bound or an explicit exemption.
    for xml in [
        &maximal_direct_debit(),
        &maximal_credit_transfer(),
        &maximal_reversal(),
        &maximal_cancellation(),
    ] {
        for (parent, element, text) in text_nodes_with_path(xml) {
            let bound = sepa::validate::max_text_len(parent.as_deref(), &element);
            match bound {
                Some(max) => assert!(
                    text.chars().count() <= max,
                    "{}/{element} is {} characters, over its Max{max}Text bound: {text:?}",
                    parent.as_deref().unwrap_or("?"),
                    text.chars().count(),
                ),
                None => assert!(
                    UNBOUNDED_BY_TYPE.contains(&element.as_str()),
                    "{}/{element} carries text that no Max*Text bound covers — add it to \
                     `validate::max_text_len` or to UNBOUNDED_BY_TYPE with a reason",
                    parent.as_deref().unwrap_or("?"),
                ),
            }
        }
    }
}
