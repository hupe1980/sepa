//! Conformance controls for lexical acceptance, fabricated values and lost observations.
//!
//! | Class | Why the existing suite could not see it | Gate here |
//! |---|---|---|
//! | The crate **refuses** input a bank may legally send | A refusal produces no document to validate, so there is nothing for a schema gate to look at | [`lexical_space`] |
//! | The crate **invents** a value the input did not carry | A fabricated value is a perfectly ordinary `Ok`; the fuzzer asserted only "does not panic" | [`no_fabrication`] |
//! | The crate **drops** something the input did carry | The output is a valid, smaller document — and one test asserted the dropping as correct | [`no_silent_drop`] |
//!
//! The oracle in the first case is the **standard's own lexical space** rather
//! than a schema: XML Schema says exactly which strings are an `xs:decimal`, an
//! `xs:date` and an `xs:dateTime`. Library representation limits, including the
//! positive four-digit year range, must be distinguished from XSD restrictions.
//! Financial expectations are independently derived from supported input fields;
//! these controls do not establish complete coverage of every ISO element.

// In tests, `unwrap()` and indexing are the assertions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;

use sepa::{
    AmountError, CreditDebitIndicator, IsoDate, IsoDateTime, ct_from_eur_str, ct_to_eur_str,
    parse_camt053,
};

// ── the standard's lexical spaces ─────────────────────────────────────────────

mod parser_contract {
    #[test]
    #[cfg(feature = "serde")]
    #[allow(clippy::too_many_lines)]
    fn scalar_routes_have_independent_acceptance_expectations() {
        fn check<T>(
            cases: &[(&str, Option<&str>)],
            constructor: fn(&str) -> Option<T>,
            wire: fn(&T) -> String,
        ) where
            T: std::str::FromStr + for<'input> TryFrom<&'input str>,
            T: serde::Serialize + serde::de::DeserializeOwned,
        {
            for &(raw, expected) in cases {
                let candidates = vec![
                    constructor(raw),
                    raw.parse::<T>().ok(),
                    T::try_from(raw).ok(),
                ];
                #[cfg(feature = "serde")]
                let candidates = {
                    let mut candidates = candidates;
                    let json = serde_json::to_string(raw).unwrap();
                    candidates.extend([
                        serde_json::from_str::<T>(&json).ok(),
                        serde_json::from_reader::<_, T>(json.as_bytes()).ok(),
                        serde_json::from_value::<T>(serde_json::json!(raw)).ok(),
                    ]);
                    candidates
                };
                for candidate in candidates {
                    assert_eq!(candidate.is_some(), expected.is_some(), "{raw:?}");
                    if let (Some(value), Some(expected)) = (candidate, expected) {
                        assert_eq!(wire(&value), expected, "{raw:?}");
                        #[cfg(feature = "serde")]
                        assert_eq!(
                            serde_json::to_value(&value).unwrap(),
                            serde_json::json!(expected)
                        );
                    }
                }
            }
        }
        check::<sepa::Iban>(
            &[
                ("DE89370400440532013000", Some("DE89370400440532013000")),
                (
                    "de89 3704 0044 0532 0130 00",
                    Some("DE89370400440532013000"),
                ),
                ("DE00370400440532013000", None),
                ("DE8937O400440532013000", None),
                ("", None),
            ],
            |raw| sepa::validate_iban(raw).ok(),
            |value| value.as_str().to_owned(),
        );
        check::<sepa::Bic>(
            &[
                ("cobadeff", Some("COBADEFF")),
                ("COBADEFFXXX", Some("COBADEFFXXX")),
                ("COBAZZFF", None),
                ("NOTPROVIDED", None),
                ("", None),
            ],
            |raw| sepa::validate_bic(raw).ok(),
            |value| value.as_str().to_owned(),
        );
        check::<sepa::CreditorId>(
            &[
                ("de98zzz09999999999", Some("DE98ZZZ09999999999")),
                ("DE00ZZZ09999999999", None),
                ("", None),
            ],
            |raw| sepa::validate_creditor_id(raw).ok(),
            |value| value.as_str().to_owned(),
        );
        check::<sepa::RfReference>(
            &[
                ("rf18 5390 0754 7034", Some("RF18539007547034")),
                ("RF00539007547034", None),
                ("", None),
            ],
            |raw| raw.parse().ok(),
            |value| value.as_str().to_owned(),
        );
        check::<sepa::Currency>(
            &[
                ("usd", Some("USD")),
                ("EUR", Some("EUR")),
                ("ZZZ", Some("ZZZ")),
                ("XXX", None),
                ("XTS", None),
                ("EURO", None),
                ("", None),
            ],
            |raw| sepa::Currency::new(raw).ok(),
            |value| value.code().to_owned(),
        );
        check::<sepa::IsoDate>(
            &[
                ("0001-01-01", Some("0001-01-01")),
                ("9999-12-31", Some("9999-12-31")),
                ("2024-02-29", Some("2024-02-29")),
                ("2023-02-29", None),
                ("0000-01-01", None),
                ("10000-01-01", None),
            ],
            |raw| sepa::IsoDate::parse(raw).ok(),
            ToString::to_string,
        );
        check::<sepa::IsoDateTime>(
            &[
                (
                    "2024-02-29T23:59:59+14:00",
                    Some("2024-02-29T23:59:59+14:00"),
                ),
                ("2024-02-29T00:00:00Z", Some("2024-02-29T00:00:00Z")),
                ("2024-02-29T24:00:00", None),
                ("2024-02-29T00:00:00+14:01", None),
                ("0000-01-01T00:00:00Z", None),
            ],
            |raw| sepa::IsoDateTime::parse(raw).ok(),
            ToString::to_string,
        );
    }

    #[test]
    fn generated_identifiers_and_calendar_arithmetic_match_known_values() {
        assert_eq!(
            sepa::Iban::from_bban("DE", "370400440532013000")
                .unwrap()
                .as_str(),
            "DE89370400440532013000"
        );
        assert!(sepa::Iban::from_bban("ZZ", "370400440532013000").is_ok());
        assert!(sepa::Iban::from_bban("Z1", "370400440532013000").is_err());
        assert!(sepa::Iban::from_bban("DE", "3704O0440532013000").is_err());
        assert!(sepa::Iban::from_bban("DE", "3704").is_err());
        assert_eq!(
            sepa::RfReference::generate("539007547034")
                .unwrap()
                .as_str(),
            "RF18539007547034"
        );
        assert_eq!(
            sepa::RfReference::generate("2348231").unwrap().as_str(),
            "RF712348231"
        );
        assert!(sepa::RfReference::generate("---").is_err());
        assert!(sepa::RfReference::generate(&"A".repeat(22)).is_err());
        for (days, expected) in [
            (-719_162, "0001-01-01"),
            (0, "1970-01-01"),
            (2_932_896, "9999-12-31"),
        ] {
            let value = sepa::IsoDate::from_epoch_days(days).unwrap();
            assert_eq!(value.to_string(), expected);
            assert_eq!(value.epoch_days(), days);
        }
        for days in [-719_163, 2_932_897, i64::MIN, i64::MAX] {
            assert!(sepa::IsoDate::from_epoch_days(days).is_err());
        }
        assert_eq!(
            sepa::IsoDate::new(2024, 2, 28)
                .unwrap()
                .plus_days(1)
                .unwrap()
                .to_string(),
            "2024-02-29"
        );
        assert!(sepa::IsoDate::MIN.plus_days(-1).is_err());
        assert!(sepa::IsoDate::MAX.plus_days(1).is_err());
        assert!(sepa::IsoDate::MIN.plus_days(i64::MIN).is_err());
        let epoch = sepa::IsoDate::new(1970, 1, 1).unwrap();
        assert_eq!(
            sepa::IsoDateTime::from(epoch).to_string(),
            "1970-01-01T00:00:00"
        );
        assert_eq!(
            sepa::IsoDateTime::from(epoch).in_utc().unix_seconds(),
            Some(0)
        );
        for (hour, minute, second) in [(24, 0, 0), (0, 60, 0), (0, 0, 60)] {
            assert!(sepa::IsoDateTime::new(epoch, hour, minute, second).is_err());
        }
        assert_eq!(
            sepa::IsoDateTime::parse("1970-01-01T01:00:00+01:00")
                .unwrap()
                .to_utc()
                .unwrap()
                .to_string(),
            "1970-01-01T00:00:00Z"
        );
        assert!(sepa::IsoDateTime::from(epoch).to_utc().is_none());
    }

    #[test]
    fn owned_scalar_and_clock_routes_keep_the_checked_domain() {
        fn check<T>(valid: &str, invalid: &str, wire: fn(&T) -> &str)
        where
            T: TryFrom<String>,
        {
            let value = T::try_from(valid.to_owned()).ok().unwrap();
            assert_eq!(wire(&value), valid);
            assert!(T::try_from(invalid.to_owned()).is_err());
        }
        check::<sepa::Iban>(
            "DE89370400440532013000",
            "DE00370400440532013000",
            sepa::Iban::as_str,
        );
        check::<sepa::Bic>("COBADEFFXXX", "COBAZZFF", sepa::Bic::as_str);
        check::<sepa::CreditorId>(
            "DE98ZZZ09999999999",
            "DE00ZZZ09999999999",
            sepa::CreditorId::as_str,
        );
        assert_eq!(
            sepa::IsoDate::parse_date_part("2024-02-29T23:59:59+14:00")
                .unwrap()
                .to_string(),
            "2024-02-29"
        );
        assert!(sepa::IsoDate::parse_date_part("2023-02-29T23:59:59Z").is_err());
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let today = sepa::IsoDate::today();
        let now = sepa::IsoDateTime::now();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(now.offset_minutes(), None);
        let seconds = u64::try_from(now.in_utc().unix_seconds().unwrap()).unwrap();
        assert!((before..=after).contains(&seconds));
        let days = u64::try_from(today.epoch_days()).unwrap();
        assert!((before / 86_400..=after / 86_400).contains(&days));
        assert!(today >= sepa::IsoDate::MIN && today <= sepa::IsoDate::MAX);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn restored_parties_require_use_time_validation() {
        let lawful =
            sepa::Party::new("End party").with_organisation_id("CUSTOMER-1", Some("CUSTOMER"));
        let lawful_json = serde_json::to_value(&lawful).unwrap();
        let mut invalid = vec![serde_json::json!({})];
        for (field, value) in [
            ("name", String::new()),
            ("name", "A".repeat(71)),
            ("id", String::new()),
            ("id", "A//B".to_owned()),
            ("id", "A".repeat(36)),
            ("id", "A\0B".to_owned()),
            ("scheme_name", String::new()),
            ("scheme_name", "A".repeat(36)),
        ] {
            let mut stored = lawful_json.clone();
            if field == "name" {
                stored[field] = value.into();
            } else {
                stored["identifier"][field] = value.into();
            }
            invalid.push(stored);
        }
        for policy in [sepa::CharsetPolicy::Strict, sepa::CharsetPolicy::default()] {
            let restored: sepa::Party = serde_json::from_value(lawful_json.clone()).unwrap();
            assert_eq!(restored, lawful);
            assert!(restored.validate("party", policy).is_ok());
            for stored in &invalid {
                let json = serde_json::to_string(stored).unwrap();
                let restored: sepa::Party = serde_json::from_reader(json.as_bytes()).unwrap();
                assert!(
                    restored.validate("party", policy).is_err(),
                    "{stored} under {policy:?}"
                );
            }
        }
        let unknown_kind = serde_json::json!({
            "identifier": {"kind": "Unknown", "id": "CUSTOMER-1"}
        });
        assert!(serde_json::from_value::<sepa::Party>(unknown_kind).is_err());
    }

    #[test]
    #[cfg(feature = "serde")]
    fn validated_scalar_restoration_agrees_with_parsing() {
        fn check<T>(inputs: &[&str])
        where
            T: std::str::FromStr
                + std::fmt::Debug
                + PartialEq
                + serde::Serialize
                + serde::de::DeserializeOwned,
        {
            assert!(inputs.first().unwrap().parse::<T>().is_ok());
            for raw in inputs {
                let parsed = raw.parse::<T>().ok();
                let json = serde_json::to_string(raw).unwrap();
                assert_eq!(serde_json::from_str::<T>(&json).ok(), parsed, "{raw:?}");
                assert_eq!(
                    serde_json::from_reader::<_, T>(json.as_bytes()).ok(),
                    parsed,
                    "{raw:?}"
                );
                if let Some(value) = parsed {
                    let json = serde_json::to_value(&value).unwrap();
                    assert!(json.is_string());
                    assert_eq!(serde_json::from_value::<T>(json).unwrap(), value);
                }
            }
            for json in ["null", "0", "[]", "{}", "[85,83,68]"] {
                assert!(serde_json::from_str::<T>(json).is_err(), "{json}");
            }
        }

        check::<sepa::Iban>(&[
            "DE89370400440532013000",
            "de89 3704 0044 0532 0130 00",
            "",
            "DE00370400440532013000",
            "DE8937O400440532013000",
        ]);
        check::<sepa::Bic>(&["COBADEFFXXX", "cobadeff", "", "NOTPROVIDED", "COBAZZFF"]);
        check::<sepa::CreditorId>(&[
            "DE98ZZZ09999999999",
            "de98zzz09999999999",
            "",
            "DE00ZZZ09999999999",
        ]);
        check::<sepa::RfReference>(&[
            "RF18539007547034",
            "rf18 5390 0754 7034",
            "",
            "RF00539007547034",
        ]);
        check::<sepa::Currency>(&["EUR", "usd", "ZZZ", "", "XXX", "XTS", "EURO", "EU\0"]);
        check::<sepa::IsoDate>(&[
            "0001-01-01",
            "9999-12-31",
            "2024-02-29",
            "2026-07-20+02:00",
            "",
            "0000-01-01",
            "10000-01-01",
            "2026-02-30",
        ]);
        check::<sepa::IsoDateTime>(&[
            "0001-01-01T00:00:00",
            "9999-12-31T23:59:59Z",
            "2026-07-20T12:30:00.123+02:00",
            "",
            "10000-01-01T00:00:00",
            "2026-07-20T25:00:00",
            "2026-07-20T12:30:00+14:01",
        ]);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn original_reference_alternatives_survive_serde_without_losing_raw_facts() {
        for raw in ["", "bad", " \n1.23\t "] {
            let reference = format!(
                "<OrgnlTxRef><IntrBkSttlmAmt Ccy='GBP'> \n5.00\t </IntrBkSttlmAmt><Amt><InstdAmt Ccy='EUR'>1.23</InstdAmt></Amt><InstdAmt Ccy='USD'>{raw}</InstdAmt></OrgnlTxRef>"
            );
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts>{reference}</TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
            );
            let document = sepa::parse_pain002(&xml).unwrap();
            let restored: sepa::pain002::Pain002Document =
                serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
            let transaction = &restored.payment_info_statuses[0].transactions[0];
            assert_eq!(transaction.original_amount_raw.as_deref(), Some("1.23"));
            let legacy = transaction.legacy_referenced_amount.as_ref().unwrap();
            assert_eq!(legacy.amount_raw.as_deref(), Some(raw));
            assert_eq!(legacy.currency.as_deref(), Some("USD"));
            assert_eq!(
                transaction.original_settlement_amount,
                document.payment_info_statuses[0].transactions[0].original_settlement_amount
            );
            assert_eq!(
                restored.financial_diagnostics(),
                document.financial_diagnostics()
            );

            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls><TxInfAndSts>{reference}</TxInfAndSts></CxlDtls></RsltnOfInvstgtn></Document>"
            );
            let document = sepa::parse_camt029(&xml).unwrap();
            let restored: sepa::camt029::Camt029Document =
                serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
            let transaction = &restored.groups[0].transactions[0];
            assert_eq!(transaction.original_amount_raw.as_deref(), Some("1.23"));
            let legacy = transaction.legacy_referenced_amount.as_ref().unwrap();
            assert_eq!(legacy.amount_raw.as_deref(), Some(raw));
            assert_eq!(legacy.currency.as_deref(), Some("USD"));
            assert_eq!(
                transaction.original_settlement_amount,
                document.groups[0].transactions[0].original_settlement_amount
            );
            assert_eq!(
                restored.financial_diagnostics(),
                document.financial_diagnostics()
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn retained_status_counts_have_independent_located_diagnostics() {
        for family in ["pain", "cancellation"] {
            for (raw, expected) in [
                (None, None),
                (Some(""), None),
                (Some("bad"), None),
                (Some("42"), Some(42)),
                (Some(" \n42\t "), Some(42)),
                (Some("18446744073709551615"), Some(u64::MAX)),
                (Some("18446744073709551616"), None),
                (Some("1.5"), None),
                (Some("-1"), None),
            ] {
                let scalar = raw.map_or_else(String::new, |raw| {
                    format!("<DtldNbOfTxs>{raw}</DtldNbOfTxs>")
                });
                let (observations, diagnostics) = if family == "pain" {
                    let original = raw.map_or_else(String::new, |raw| {
                        format!("<OrgnlNbOfTxs>{raw}</OrgnlNbOfTxs>")
                    });
                    let bucket = format!("<NbOfTxsPerSts>{scalar}</NbOfTxsPerSts>");
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId>{original}{bucket}</OrgnlGrpInfAndSts><OrgnlPmtInfAndSts>{original}{bucket}</OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
                    );
                    let document = sepa::parse_pain002(&xml).unwrap();
                    let group = &document.group_status_counts[0];
                    let info = &document.payment_info_statuses[0].status_counts[0];
                    (
                        vec![
                            (
                                "OrgnlGrpInfAndSts/OrgnlNbOfTxs",
                                document.original_number_of_transactions_raw.clone(),
                                document.original_number_of_transactions,
                            ),
                            (
                                "OrgnlPmtInfAndSts[0]/OrgnlNbOfTxs",
                                document.payment_info_statuses[0]
                                    .original_number_of_transactions_raw
                                    .clone(),
                                document.payment_info_statuses[0].original_number_of_transactions,
                            ),
                            (
                                "OrgnlGrpInfAndSts/NbOfTxsPerSts[0]/DtldNbOfTxs",
                                group.count_raw.clone(),
                                group.count,
                            ),
                            (
                                "OrgnlPmtInfAndSts[0]/NbOfTxsPerSts[0]/DtldNbOfTxs",
                                info.count_raw.clone(),
                                info.count,
                            ),
                        ],
                        document.financial_diagnostics(),
                    )
                } else {
                    let original = raw.map_or_else(String::new, |raw| {
                        format!("<OrgnlNbOfTxs>{raw}</OrgnlNbOfTxs>")
                    });
                    let bucket = format!("<NbOfTxsPerCxlSts>{scalar}</NbOfTxsPerCxlSts>");
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls><OrgnlGrpInfAndSts>{original}{bucket}</OrgnlGrpInfAndSts><OrgnlPmtInfAndSts>{bucket}</OrgnlPmtInfAndSts></CxlDtls></RsltnOfInvstgtn></Document>"
                    );
                    let document = sepa::parse_camt029(&xml).unwrap();
                    let group = &document.groups[0];
                    let count = &group.status_counts[0];
                    let info = &group.payment_infos[0].status_counts[0];
                    (
                        vec![
                            (
                                "CxlDtls[0]/OrgnlGrpInfAndSts/OrgnlNbOfTxs",
                                group.original_number_of_transactions_raw.clone(),
                                group.original_number_of_transactions,
                            ),
                            (
                                "CxlDtls[0]/OrgnlGrpInfAndSts/NbOfTxsPerCxlSts[0]/DtldNbOfTxs",
                                count.count_raw.clone(),
                                count.count,
                            ),
                            (
                                "CxlDtls[0]/OrgnlPmtInfAndSts[0]/NbOfTxsPerCxlSts[0]/DtldNbOfTxs",
                                info.count_raw.clone(),
                                info.count,
                            ),
                        ],
                        document.financial_diagnostics(),
                    )
                };
                for (path, observed_raw, count) in observations {
                    assert_eq!(observed_raw.as_deref(), raw);
                    assert_eq!(count, expected);
                    let located: Vec<_> = diagnostics
                        .iter()
                        .filter(|issue| issue.path == path)
                        .collect();
                    if raw.is_some() && expected.is_none() {
                        assert_eq!(located.len(), 1, "{family}/{path}/{raw:?}");
                        assert_eq!(located[0].raw.as_deref(), raw);
                        assert_eq!(
                            located[0].issue,
                            if raw == Some("18446744073709551616") {
                                sepa::camt::FinancialIssue::Overflow
                            } else {
                                sepa::camt::FinancialIssue::InvalidCount
                            }
                        );
                    } else {
                        assert_eq!(located.len(), 0, "{family}/{path}/{raw:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn financial_count_diagnostics_recheck_current_typed_observations() {
        use sepa::camt::FinancialIssue;
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><NbOfTxsPerSts><DtldNbOfTxs>42</DtldNbOfTxs></NbOfTxsPerSts></OrgnlGrpInfAndSts></CstmrPmtStsRpt></Document>";
        let mut document = sepa::parse_pain002(xml).unwrap();
        assert_eq!(document.financial_diagnostics().len(), 0);
        for count in [Some(43), None] {
            document.group_status_counts[0].count = count;
            let diagnostics = document.financial_diagnostics();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].issue, FinancialIssue::InconsistentValue);
            assert_eq!(
                diagnostics[0].path,
                "OrgnlGrpInfAndSts/NbOfTxsPerSts[0]/DtldNbOfTxs"
            );
            assert_eq!(diagnostics[0].raw.as_deref(), Some("42"));
        }
        document.group_status_counts[0].count = Some(42);
        document.group_status_counts[0].count_raw = None;
        let diagnostics = document.financial_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].issue, FinancialIssue::InconsistentValue);
        assert_eq!(diagnostics[0].raw, None);
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn original_settlement_amount_is_independent_of_instructed_amount() {
        use sepa::camt::FinancialIssue;
        for family in ["pain", "cancellation_group", "cancellation_info"] {
            for instructed in ["", "<Amt><InstdAmt Ccy='EUR'>1.23</InstdAmt></Amt>"] {
                for (raw, expected, issue) in [
                    (None, None, None),
                    (Some(""), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("bad"), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("5.00"), Some(500), None),
                    (Some(" \n5.00\t "), Some(500), None),
                    (Some("0.001"), None, Some(FinancialIssue::SubCentPrecision)),
                    (
                        Some("999999999999999999999"),
                        None,
                        Some(FinancialIssue::Overflow),
                    ),
                    (Some("-1.00"), None, Some(FinancialIssue::NegativeAmount)),
                ] {
                    let settlement = raw.map_or_else(String::new, |raw| {
                        format!("<IntrBkSttlmAmt Ccy='USD'>{raw}</IntrBkSttlmAmt>")
                    });
                    let reference = format!("<OrgnlTxRef>{settlement}{instructed}</OrgnlTxRef>");
                    let (amount, diagnostics, base) = if family == "pain" {
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts>{reference}</TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
                        );
                        let document = sepa::parse_pain002(&xml).unwrap();
                        let transaction = &document.payment_info_statuses[0].transactions[0];
                        assert_eq!(
                            transaction.original_amount_ct,
                            if instructed.is_empty() {
                                None
                            } else {
                                Some(123)
                            }
                        );
                        (
                            transaction.original_settlement_amount.clone(),
                            document.financial_diagnostics(),
                            "OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                        )
                    } else {
                        let transaction = format!("<TxInfAndSts>{reference}</TxInfAndSts>");
                        let content = if family == "cancellation_info" {
                            format!("<OrgnlPmtInfAndSts>{transaction}</OrgnlPmtInfAndSts>")
                        } else {
                            transaction
                        };
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls>{content}</CxlDtls></RsltnOfInvstgtn></Document>"
                        );
                        let document = sepa::parse_camt029(&xml).unwrap();
                        let (transaction, base) = if family == "cancellation_info" {
                            (
                                &document.groups[0].payment_infos[0].transactions[0],
                                "CxlDtls[0]/OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                            )
                        } else {
                            (
                                &document.groups[0].transactions[0],
                                "CxlDtls[0]/TxInfAndSts[0]",
                            )
                        };
                        assert_eq!(
                            transaction.original_amount_ct,
                            if instructed.is_empty() {
                                None
                            } else {
                                Some(123)
                            }
                        );
                        (
                            transaction.original_settlement_amount.clone(),
                            document.financial_diagnostics(),
                            base,
                        )
                    };
                    if let Some(raw) = raw {
                        let amount = amount.unwrap();
                        assert_eq!(amount.amount_raw.as_deref(), Some(raw));
                        assert_eq!(amount.currency.as_deref(), Some("USD"));
                        assert_eq!(amount.ct, expected);
                    } else {
                        assert!(amount.is_none());
                    }
                    assert_eq!(diagnostics.len(), usize::from(issue.is_some()));
                    if let Some(issue) = issue {
                        assert_eq!(diagnostics[0].issue, issue);
                        assert_eq!(
                            diagnostics[0].path,
                            format!("{base}/OrgnlTxRef/IntrBkSttlmAmt")
                        );
                        assert_eq!(diagnostics[0].raw.as_deref(), raw);
                    }
                }
            }
        }
    }

    #[test]
    fn original_reference_fallback_depends_on_scalar_not_container_presence() {
        use sepa::camt::{FinancialIssue, OriginalAmountSource};
        for family in ["pain", "cancellation_group", "cancellation_info"] {
            for container in [
                "",
                "<Amt/>",
                "<Amt><EqvtAmt><Amt Ccy='USD'>2.00</Amt><CcyOfTrf>CHF</CcyOfTrf></EqvtAmt></Amt>",
            ] {
                for legacy in ["", "<InstdAmt Ccy='EUR'>1.23</InstdAmt>"] {
                    let reference = format!("<OrgnlTxRef>{container}{legacy}</OrgnlTxRef>");
                    let (raw, amount, source, equivalent, diagnostics, base) = if family == "pain" {
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts>{reference}</TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
                        );
                        let document = sepa::parse_pain002(&xml).unwrap();
                        let transaction = &document.payment_info_statuses[0].transactions[0];
                        assert!(transaction.legacy_referenced_amount.is_none());
                        (
                            transaction.original_amount_raw.clone(),
                            transaction.original_amount_ct,
                            transaction.original_amount_source,
                            transaction.original_equivalent_amount.clone(),
                            document.financial_diagnostics(),
                            "OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                        )
                    } else {
                        let transaction = format!("<TxInfAndSts>{reference}</TxInfAndSts>");
                        let content = if family == "cancellation_info" {
                            format!("<OrgnlPmtInfAndSts>{transaction}</OrgnlPmtInfAndSts>")
                        } else {
                            transaction
                        };
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls>{content}</CxlDtls></RsltnOfInvstgtn></Document>"
                        );
                        let document = sepa::parse_camt029(&xml).unwrap();
                        let (transaction, base) = if family == "cancellation_info" {
                            (
                                &document.groups[0].payment_infos[0].transactions[0],
                                "CxlDtls[0]/OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                            )
                        } else {
                            (
                                &document.groups[0].transactions[0],
                                "CxlDtls[0]/TxInfAndSts[0]",
                            )
                        };
                        assert!(transaction.legacy_referenced_amount.is_none());
                        assert_eq!(transaction.referenced_amount.is_some(), !legacy.is_empty());
                        (
                            transaction.original_amount_raw.clone(),
                            transaction.original_amount_ct,
                            transaction.original_amount_source,
                            transaction.referenced_equivalent_amount.clone(),
                            document.financial_diagnostics(),
                            base,
                        )
                    };
                    if legacy.is_empty() {
                        assert_eq!(raw, None);
                        assert_eq!(amount, None);
                        assert_eq!(source, None);
                    } else {
                        assert_eq!(raw.as_deref(), Some("1.23"), "{family}/{container}");
                        assert_eq!(amount, Some(123));
                        assert_eq!(
                            source,
                            Some(OriginalAmountSource::LegacyTransactionReference)
                        );
                    }
                    if container.contains("EqvtAmt") {
                        let equivalent = equivalent.unwrap();
                        assert_eq!(equivalent.amount_raw.as_deref(), Some("2.00"));
                        assert_eq!(equivalent.ct, Some(200));
                        assert_eq!(equivalent.currency.as_deref(), Some("USD"));
                        assert_eq!(
                            diagnostics.iter().any(|diagnostic| {
                                diagnostic.issue == FinancialIssue::ConflictingAlternatives
                                    && diagnostic.path.starts_with(base)
                            }),
                            !legacy.is_empty()
                        );
                    } else {
                        assert!(equivalent.is_none());
                        assert_eq!(diagnostics.len(), 0);
                    }
                }
            }
        }
    }

    #[test]
    fn original_reference_routes_preserve_simultaneous_legacy_observations() {
        use sepa::camt::FinancialIssue;
        for (family, currency) in [
            ("pain", "EUR"),
            ("pain", "USD"),
            ("cancellation_group", "EUR"),
            ("cancellation_group", "USD"),
            ("cancellation_info", "EUR"),
            ("cancellation_info", "USD"),
        ] {
            for (raw, expected_ct, issue) in [
                ("", None, Some(FinancialIssue::InvalidAmount)),
                ("bad", None, Some(FinancialIssue::InvalidAmount)),
                ("1.23", Some(123), None),
                ("2.00", Some(200), None),
                (" \n1.23\t ", Some(123), None),
                ("0.001", None, Some(FinancialIssue::SubCentPrecision)),
                (
                    "999999999999999999999",
                    None,
                    Some(FinancialIssue::Overflow),
                ),
                ("-1.00", None, Some(FinancialIssue::NegativeAmount)),
            ] {
                let reference = format!(
                    "<OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>1.23</InstdAmt></Amt><InstdAmt Ccy='{currency}'>{raw}</InstdAmt></OrgnlTxRef>"
                );
                let (selected, legacy, diagnostics, base) = if family == "pain" {
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts>{reference}</TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
                    );
                    let document = sepa::parse_pain002(&xml).unwrap();
                    let transaction = &document.payment_info_statuses[0].transactions[0];
                    (
                        transaction.original_amount_ct,
                        transaction.legacy_referenced_amount.clone(),
                        document.financial_diagnostics(),
                        "OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                    )
                } else {
                    let transaction = format!("<TxInfAndSts>{reference}</TxInfAndSts>");
                    let content = if family == "cancellation_info" {
                        format!("<OrgnlPmtInfAndSts>{transaction}</OrgnlPmtInfAndSts>")
                    } else {
                        transaction
                    };
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls>{content}</CxlDtls></RsltnOfInvstgtn></Document>"
                    );
                    let document = sepa::parse_camt029(&xml).unwrap();
                    let (transaction, base) = if family == "cancellation_info" {
                        (
                            &document.groups[0].payment_infos[0].transactions[0],
                            "CxlDtls[0]/OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                        )
                    } else {
                        (
                            &document.groups[0].transactions[0],
                            "CxlDtls[0]/TxInfAndSts[0]",
                        )
                    };
                    (
                        transaction.original_amount_ct,
                        transaction.legacy_referenced_amount.clone(),
                        document.financial_diagnostics(),
                        base,
                    )
                };
                assert_eq!(selected, Some(123));
                let legacy = legacy.unwrap();
                assert_eq!(legacy.amount_raw.as_deref(), Some(raw));
                assert_eq!(legacy.ct, expected_ct);
                assert_eq!(legacy.currency.as_deref(), Some(currency));
                assert!(diagnostics.iter().any(|diagnostic| {
                    diagnostic.issue == FinancialIssue::ConflictingAlternatives
                        && diagnostic.path.starts_with(base)
                }));
                if let Some(issue) = issue {
                    assert!(diagnostics.iter().any(|diagnostic| {
                        diagnostic.issue == issue
                            && diagnostic.path == format!("{base}/OrgnlTxRef/InstdAmt")
                            && diagnostic.raw.as_deref() == Some(raw)
                    }));
                }
            }
        }
    }

    #[test]
    fn financial_diagnostics_expose_aggregate_and_choice_disagreements() {
        use sepa::camt::FinancialIssue;
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.053.001.08'><BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Chrgs><TtlChrgsAndTaxAmt Ccy='EUR'>3.00</TtlChrgsAndTaxAmt><Rcrd><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd><Rcrd><Amt Ccy='USD'>1.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs></Ntry></Stmt></BkToCstmrStmt></Document>";
        let document = sepa::parse_camt053(xml).unwrap();
        let diagnostics = document.financial_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.issue
            == FinancialIssue::CurrencyMismatch
            && diagnostic.path.ends_with("/Rcrd[1]/Amt/@Ccy")));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.issue != FinancialIssue::TotalMismatch)
        );
        let comparable = sepa::parse_camt053(&xml.replace("Ccy='USD'", "Ccy='EUR'")).unwrap();
        let diagnostics = comparable.financial_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.issue
            == FinancialIssue::TotalMismatch
            && diagnostic.raw.as_deref() == Some("3.00")));
        let mut inconsistent = comparable;
        inconsistent.statements[0].entries[0]
            .charges
            .as_mut()
            .unwrap()
            .records[0]
            .amount
            .ct = Some(300);
        let diagnostics = inconsistent.financial_diagnostics();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.issue == FinancialIssue::InconsistentValue)
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.issue != FinancialIssue::TotalMismatch)
        );
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls><TxInfAndSts><OrgnlInstdAmt Ccy='EUR'>1.00</OrgnlInstdAmt><OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>2.00</InstdAmt><EqvtAmt><Amt Ccy='USD'>3.00</Amt><CcyOfTrf>CHF</CcyOfTrf></EqvtAmt></Amt></OrgnlTxRef></TxInfAndSts></CxlDtls></RsltnOfInvstgtn></Document>";
        let document = sepa::parse_camt029(xml).unwrap();
        assert_eq!(
            document
                .financial_diagnostics()
                .iter()
                .filter(|diagnostic| diagnostic.issue == FinancialIssue::ConflictingAlternatives)
                .count(),
            2
        );
    }

    #[test]
    fn equivalent_transfer_currency_diagnostics_cover_both_status_readers() {
        use sepa::camt::FinancialIssue;
        for (family, body, group) in [
            ("pain.002.001.10", "CstmrPmtStsRpt", "OrgnlPmtInfAndSts"),
            ("camt.029.001.06", "RsltnOfInvstgtn", "CxlDtls"),
        ] {
            for (currency, expected) in [
                ("<CcyOfTrf>CHF</CcyOfTrf>", None),
                (
                    "<CcyOfTrf>eur</CcyOfTrf>",
                    Some(FinancialIssue::InvalidCurrency),
                ),
                (
                    "<CcyOfTrf></CcyOfTrf>",
                    Some(FinancialIssue::InvalidCurrency),
                ),
                ("", Some(FinancialIssue::MissingCurrency)),
            ] {
                let header = if family.starts_with("pain") {
                    "<GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId></OrgnlGrpInfAndSts>"
                } else {
                    "<Assgnmt><Id>A</Id></Assgnmt>"
                };
                let xml = format!(
                    "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}'><{body}>{header}<{group}><TxInfAndSts><OrgnlTxRef><Amt><EqvtAmt><Amt Ccy='USD'>1.00</Amt>{currency}</EqvtAmt></Amt></OrgnlTxRef></TxInfAndSts></{group}></{body}></Document>"
                );
                let diagnostics = if family.starts_with("pain") {
                    sepa::parse_pain002(&xml).unwrap().financial_diagnostics()
                } else {
                    sepa::parse_camt029(&xml).unwrap().financial_diagnostics()
                };
                if let Some(expected) = expected {
                    assert_eq!(diagnostics.len(), 1);
                    assert_eq!(diagnostics[0].issue, expected);
                    assert!(diagnostics[0].path.ends_with("/EqvtAmt/CcyOfTrf"));
                } else {
                    assert_eq!(diagnostics.len(), 0);
                }
            }
        }
    }

    #[test]
    fn status_diagnostics_keep_exact_control_sums_and_observed_source_routes() {
        use sepa::camt::{FinancialIssue, OriginalAmountSource};
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId><NbOfTxsPerSts><DtldSts>ACTC</DtldSts><DtldCtrlSum>-1.00</DtldCtrlSum></NbOfTxsPerSts></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts><OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>1.00</InstdAmt></Amt></OrgnlTxRef></TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>";
        let lawful = sepa::parse_pain002(xml).unwrap();
        assert_eq!(lawful.financial_diagnostics().len(), 0);
        assert_eq!(
            lawful.payment_info_statuses[0].transactions[0].original_amount_source,
            Some(OriginalAmountSource::TransactionReference)
        );
        let invalid =
            sepa::parse_pain002(&xml.replace("Ccy='EUR'>1.00", "Ccy='EUR'>0.001")).unwrap();
        let diagnostics = invalid.financial_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].issue, FinancialIssue::SubCentPrecision);
        assert!(
            diagnostics[0]
                .path
                .contains("OrgnlPmtInfAndSts[0]/TxInfAndSts[0]")
        );
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls><OrgnlPmtInfAndSts><TxInfAndSts><OrgnlInstdAmt Ccy='EUR'>bad</OrgnlInstdAmt><OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>bad</InstdAmt></Amt></OrgnlTxRef></TxInfAndSts></OrgnlPmtInfAndSts></CxlDtls></RsltnOfInvstgtn></Document>";
        let document = sepa::parse_camt029(xml).unwrap();
        let diagnostics = document.financial_diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].path.ends_with("/OrgnlInstdAmt"));
        assert!(diagnostics[1].path.ends_with("/OrgnlTxRef/Amt/InstdAmt"));
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.issue
            == FinancialIssue::InvalidAmount
            && diagnostic.raw.as_deref() == Some("bad")));
        let reference_only =
            sepa::parse_camt029(&xml.replace("<OrgnlInstdAmt Ccy='EUR'>bad</OrgnlInstdAmt>", ""))
                .unwrap();
        assert_eq!(reference_only.financial_diagnostics().len(), 1);
        assert_eq!(
            reference_only.groups[0].payment_infos[0].transactions[0].original_amount_source,
            Some(OriginalAmountSource::TransactionReference)
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn status_amount_currencies_preserve_presence_and_location() {
        use sepa::camt::FinancialIssue;
        for family in ["pain", "cancellation_group", "cancellation_info"] {
            for field in [
                "instructed",
                "legacy",
                "equivalent",
                "settlement",
                "original",
            ] {
                if family == "pain" && field == "original" {
                    continue;
                }
                for (currency, issue) in [
                    (None, Some(FinancialIssue::MissingCurrency)),
                    (Some(""), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EU"), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EUR"), None),
                ] {
                    let attribute =
                        currency.map_or_else(String::new, |raw| format!(" Ccy='{raw}'"));
                    let amount = format!("<InstdAmt{attribute}>1.23</InstdAmt>");
                    let (content, suffix) = match field {
                        "instructed" => (
                            format!("<OrgnlTxRef><Amt>{amount}</Amt></OrgnlTxRef>"),
                            "OrgnlTxRef/Amt/InstdAmt",
                        ),
                        "legacy" => (
                            format!(
                                "<OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>1.00</InstdAmt></Amt>{amount}</OrgnlTxRef>"
                            ),
                            "OrgnlTxRef/InstdAmt",
                        ),
                        "equivalent" => (
                            format!(
                                "<OrgnlTxRef><Amt><EqvtAmt><Amt{attribute}>1.23</Amt><CcyOfTrf>USD</CcyOfTrf></EqvtAmt></Amt></OrgnlTxRef>"
                            ),
                            "OrgnlTxRef/Amt/EqvtAmt/Amt",
                        ),
                        "settlement" => (
                            format!(
                                "<OrgnlTxRef><IntrBkSttlmAmt{attribute}>1.23</IntrBkSttlmAmt></OrgnlTxRef>"
                            ),
                            "OrgnlTxRef/IntrBkSttlmAmt",
                        ),
                        _ => (
                            format!("<OrgnlInstdAmt{attribute}>1.23</OrgnlInstdAmt>"),
                            "OrgnlInstdAmt",
                        ),
                    };
                    let (raw, cents, observed_currency, diagnostics, base) = if family == "pain" {
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts>{content}</TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
                        );
                        let document = sepa::parse_pain002(&xml).unwrap();
                        let transaction = &document.payment_info_statuses[0].transactions[0];
                        let observed = match field {
                            "legacy" => transaction.legacy_referenced_amount.as_ref(),
                            "equivalent" => transaction.original_equivalent_amount.as_ref(),
                            "settlement" => transaction.original_settlement_amount.as_ref(),
                            _ => None,
                        };
                        let values = if let Some(observed) = observed {
                            (
                                observed.amount_raw.clone(),
                                observed.ct,
                                observed.currency.clone(),
                            )
                        } else {
                            (
                                transaction.original_amount_raw.clone(),
                                transaction.original_amount_ct,
                                transaction.original_currency.clone(),
                            )
                        };
                        (
                            values.0,
                            values.1,
                            values.2,
                            document.financial_diagnostics(),
                            "OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                        )
                    } else {
                        let content = format!("<TxInfAndSts>{content}</TxInfAndSts>");
                        let content = if family == "cancellation_info" {
                            format!("<OrgnlPmtInfAndSts>{content}</OrgnlPmtInfAndSts>")
                        } else {
                            content
                        };
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls>{content}</CxlDtls></RsltnOfInvstgtn></Document>"
                        );
                        let document = sepa::parse_camt029(&xml).unwrap();
                        let (transaction, base) = if family == "cancellation_info" {
                            (
                                &document.groups[0].payment_infos[0].transactions[0],
                                "CxlDtls[0]/OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
                            )
                        } else {
                            (
                                &document.groups[0].transactions[0],
                                "CxlDtls[0]/TxInfAndSts[0]",
                            )
                        };
                        let observed = match field {
                            "legacy" => transaction.legacy_referenced_amount.as_ref(),
                            "equivalent" => transaction.referenced_equivalent_amount.as_ref(),
                            "settlement" => transaction.original_settlement_amount.as_ref(),
                            "instructed" => transaction.referenced_amount.as_ref(),
                            _ => None,
                        };
                        let values = if let Some(observed) = observed {
                            (
                                observed.amount_raw.clone(),
                                observed.ct,
                                observed.currency.clone(),
                            )
                        } else {
                            (
                                transaction.original_amount_raw.clone(),
                                transaction.original_amount_ct,
                                transaction.original_currency.clone(),
                            )
                        };
                        (
                            values.0,
                            values.1,
                            values.2,
                            document.financial_diagnostics(),
                            base,
                        )
                    };
                    assert_eq!(raw.as_deref(), Some("1.23"), "{family}/{field}");
                    assert_eq!(cents, Some(123), "{family}/{field}");
                    assert_eq!(observed_currency.as_deref(), currency, "{family}/{field}");
                    let path = format!("{base}/{suffix}/@Ccy");
                    let primary: Vec<_> = diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic.path == path
                                && matches!(
                                    diagnostic.issue,
                                    FinancialIssue::MissingCurrency
                                        | FinancialIssue::InvalidCurrency
                                )
                        })
                        .collect();
                    assert_eq!(
                        primary.len(),
                        usize::from(issue.is_some()),
                        "{family}/{field}/{currency:?}"
                    );
                    if let Some(issue) = issue {
                        assert_eq!(primary[0].issue, issue);
                        assert_eq!(primary[0].raw.as_deref(), currency);
                    }
                }
            }
        }
    }

    #[test]
    fn status_financial_field_matrix_retains_every_magnitude_state() {
        use sepa::camt::FinancialIssue;
        for (family, field) in [
            ("pain", "control_original_group"),
            ("pain", "control_original_info"),
            ("pain", "control_group"),
            ("pain", "control_info"),
            ("pain", "canonical"),
            ("pain", "legacy"),
            ("pain", "equivalent"),
            ("cancellation", "control_original"),
            ("cancellation", "control_group"),
            ("cancellation", "control_info"),
            ("cancellation", "direct"),
            ("cancellation", "canonical"),
            ("cancellation", "legacy"),
            ("cancellation", "equivalent"),
            ("cancellation", "info_direct"),
            ("cancellation", "info_canonical"),
            ("cancellation", "info_legacy"),
            ("cancellation", "info_equivalent"),
        ] {
            for (raw, expected_ct, expected_issue) in [
                (None, None, None),
                (Some(""), None, Some(FinancialIssue::InvalidAmount)),
                (Some("bad"), None, Some(FinancialIssue::InvalidAmount)),
                (Some("1.23"), Some(123), None),
                (Some(" \n1.23\t "), Some(123), None),
                (Some("0.001"), None, Some(FinancialIssue::SubCentPrecision)),
                (
                    Some("999999999999999999999"),
                    None,
                    Some(FinancialIssue::Overflow),
                ),
                (Some("-1.00"), None, Some(FinancialIssue::NegativeAmount)),
            ] {
                let (observed_raw, observed_ct, diagnostics, path) =
                    status_matrix_observations(family, field, raw);
                let control = field.starts_with("control_");
                let expected_ct = if control && raw == Some("-1.00") {
                    Some(-100)
                } else {
                    expected_ct
                };
                let expected_issue = if control && raw == Some("-1.00") {
                    None
                } else {
                    expected_issue
                };
                assert_eq!(observed_raw.as_deref(), raw, "{family}/{field}");
                assert_eq!(observed_ct, expected_ct, "{family}/{field}/{raw:?}");
                let located: Vec<_> = diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.path == path)
                    .collect();
                if let Some(expected) = expected_issue {
                    assert_eq!(located.len(), 1, "{family}/{field}/{raw:?}");
                    assert_eq!(located[0].issue, expected);
                    assert_eq!(located[0].raw.as_deref(), raw);
                } else {
                    assert_eq!(located.len(), 0, "{family}/{field}/{raw:?}");
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn status_matrix_observations(
        family: &str,
        field: &str,
        raw: Option<&str>,
    ) -> (
        Option<String>,
        Option<i64>,
        Vec<sepa::camt::FinancialDiagnostic>,
        String,
    ) {
        let control = field.starts_with("control_");
        let route = field.strip_prefix("info_").unwrap_or(field);
        let (fragment, suffix) = match route {
            "control_group" | "control_info" => (raw.map_or_else(String::new, |raw| format!("<DtldCtrlSum>{raw}</DtldCtrlSum>")), "DtldCtrlSum"),
            "control_original" | "control_original_group" | "control_original_info" => (raw.map_or_else(String::new, |raw| format!("<OrgnlCtrlSum>{raw}</OrgnlCtrlSum>")), "OrgnlCtrlSum"),
            "direct" => (raw.map_or_else(String::new, |raw| format!("<OrgnlInstdAmt Ccy='EUR'>{raw}</OrgnlInstdAmt>")), "OrgnlInstdAmt"),
            "legacy" => (raw.map_or_else(String::new, |raw| format!("<OrgnlTxRef><InstdAmt Ccy='EUR'>{raw}</InstdAmt></OrgnlTxRef>")), "OrgnlTxRef/InstdAmt"),
            "equivalent" => (raw.map_or_else(String::new, |raw| format!("<OrgnlTxRef><Amt><EqvtAmt><Amt Ccy='EUR'>{raw}</Amt><CcyOfTrf>CHF</CcyOfTrf></EqvtAmt></Amt></OrgnlTxRef>")), "OrgnlTxRef/Amt/EqvtAmt/Amt"),
            _ => (raw.map_or_else(String::new, |raw| format!("<OrgnlTxRef><Amt><InstdAmt Ccy='EUR'>{raw}</InstdAmt></Amt></OrgnlTxRef>")), "OrgnlTxRef/Amt/InstdAmt"),
        };
        if family == "pain" {
            let counts = format!("<NbOfTxsPerSts>{fragment}</NbOfTxsPerSts>");
            let group = if field == "control_group" {
                counts.as_str()
            } else if field == "control_original_group" {
                fragment.as_str()
            } else {
                ""
            };
            let info = if field == "control_info" {
                counts.clone()
            } else if field == "control_original_info" {
                fragment.clone()
            } else if control {
                String::new()
            } else {
                format!("<TxInfAndSts>{fragment}</TxInfAndSts>")
            };
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId>{group}</OrgnlGrpInfAndSts><OrgnlPmtInfAndSts>{info}</OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
            );
            let document = sepa::parse_pain002(&xml).unwrap();
            let diagnostics = document.financial_diagnostics();
            if field == "control_original_group" {
                return (
                    document.original_control_sum_raw.clone(),
                    document.original_control_sum_ct,
                    diagnostics,
                    format!("OrgnlGrpInfAndSts/{suffix}"),
                );
            }
            if field == "control_group" {
                let count = &document.group_status_counts[0];
                return (
                    count.total_raw.clone(),
                    count.total_ct,
                    diagnostics,
                    format!("OrgnlGrpInfAndSts/NbOfTxsPerSts[0]/{suffix}"),
                );
            }
            let info = &document.payment_info_statuses[0];
            if field == "control_original_info" {
                return (
                    info.original_control_sum_raw.clone(),
                    info.original_control_sum_ct,
                    diagnostics,
                    format!("OrgnlPmtInfAndSts[0]/{suffix}"),
                );
            }
            if control {
                let count = &info.status_counts[0];
                return (
                    count.total_raw.clone(),
                    count.total_ct,
                    diagnostics,
                    format!("OrgnlPmtInfAndSts[0]/NbOfTxsPerSts[0]/{suffix}"),
                );
            }
            let transaction = &info.transactions[0];
            let path = format!("OrgnlPmtInfAndSts[0]/TxInfAndSts[0]/{suffix}");
            return if route == "equivalent" {
                let amount = transaction.original_equivalent_amount.as_ref();
                (
                    amount.and_then(|amount| amount.amount_raw.clone()),
                    amount.and_then(|amount| amount.ct),
                    diagnostics,
                    path,
                )
            } else {
                (
                    transaction.original_amount_raw.clone(),
                    transaction.original_amount_ct,
                    diagnostics,
                    path,
                )
            };
        }
        let counts = format!("<NbOfTxsPerCxlSts>{fragment}</NbOfTxsPerCxlSts>");
        let content = match field {
            "control_original" => format!("<OrgnlGrpInfAndSts>{fragment}</OrgnlGrpInfAndSts>"),
            "control_group" => format!("<OrgnlGrpInfAndSts>{counts}</OrgnlGrpInfAndSts>"),
            "control_info" => format!("<OrgnlPmtInfAndSts>{counts}</OrgnlPmtInfAndSts>"),
            _ if field.starts_with("info_") => format!(
                "<OrgnlPmtInfAndSts><TxInfAndSts>{fragment}</TxInfAndSts></OrgnlPmtInfAndSts>"
            ),
            _ => format!("<TxInfAndSts>{fragment}</TxInfAndSts>"),
        };
        let xml = format!(
            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls>{content}</CxlDtls></RsltnOfInvstgtn></Document>"
        );
        let document = sepa::parse_camt029(&xml).unwrap();
        let diagnostics = document.financial_diagnostics();
        let group = &document.groups[0];
        if field == "control_original" {
            return (
                group.original_control_sum_raw.clone(),
                group.original_control_sum_ct,
                diagnostics,
                format!("CxlDtls[0]/OrgnlGrpInfAndSts/{suffix}"),
            );
        }
        if field == "control_group" {
            let count = &group.status_counts[0];
            return (
                count.control_sum_raw.clone(),
                count.control_sum_ct,
                diagnostics,
                format!("CxlDtls[0]/OrgnlGrpInfAndSts/NbOfTxsPerCxlSts[0]/{suffix}"),
            );
        }
        if field == "control_info" {
            let count = &group.payment_infos[0].status_counts[0];
            return (
                count.control_sum_raw.clone(),
                count.control_sum_ct,
                diagnostics,
                format!("CxlDtls[0]/OrgnlPmtInfAndSts[0]/NbOfTxsPerCxlSts[0]/{suffix}"),
            );
        }
        let (transaction, base) = if field.starts_with("info_") {
            (
                &group.payment_infos[0].transactions[0],
                "CxlDtls[0]/OrgnlPmtInfAndSts[0]/TxInfAndSts[0]",
            )
        } else {
            (&group.transactions[0], "CxlDtls[0]/TxInfAndSts[0]")
        };
        let path = format!("{base}/{suffix}");
        if route == "equivalent" {
            let amount = transaction.referenced_equivalent_amount.as_ref();
            (
                amount.and_then(|amount| amount.amount_raw.clone()),
                amount.and_then(|amount| amount.ct),
                diagnostics,
                path,
            )
        } else {
            (
                transaction.original_amount_raw.clone(),
                transaction.original_amount_ct,
                diagnostics,
                path,
            )
        }
    }

    #[test]
    fn mixed_charge_shapes_are_retained_but_not_aggregated() {
        use sepa::camt::{ChargeRecordSource, FinancialIssue};
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.053.001.08'><BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Chrgs><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><Rcrd><Amt Ccy='EUR'>bad</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs></Ntry></Stmt></BkToCstmrStmt></Document>";
        let document = sepa::parse_camt053(xml).unwrap();
        let charges = document.statements[0].entries[0].charges.as_ref().unwrap();
        assert_eq!(charges.records.len(), 2);
        assert_eq!(charges.records[0].source, Some(ChargeRecordSource::Record));
        assert_eq!(
            charges.records[1].source,
            Some(ChargeRecordSource::LegacyFlat)
        );
        assert_eq!(
            charges.records[1].amount.amount_raw.as_deref(),
            Some("1.00")
        );
        assert_eq!(charges.total_signed_ct(), None);
        let diagnostics = document.financial_diagnostics();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.issue
            == FinancialIssue::InvalidAmount
            && diagnostic.path.ends_with("/Chrgs/Rcrd[0]/Amt")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.issue
            == FinancialIssue::ConflictingAlternatives
            && diagnostic.path.ends_with("/Chrgs")));
        let readable = sepa::parse_camt053(&xml.replace(">bad<", ">1.00<")).unwrap();
        assert_eq!(
            readable.statements[0].entries[0]
                .charges
                .as_ref()
                .unwrap()
                .total_signed_ct(),
            None
        );
    }

    #[test]
    fn legacy_flat_charge_matrix_preserves_actual_routes() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            for level in ["entry", "detail"] {
                for (raw, expected_ct, expected_issue) in [
                    (None, None, None),
                    (Some(""), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("bad"), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("1.23"), Some(123), None),
                    (Some(" \n1.23\t "), Some(123), None),
                    (Some("0.001"), None, Some(FinancialIssue::SubCentPrecision)),
                    (
                        Some("999999999999999999999"),
                        None,
                        Some(FinancialIssue::Overflow),
                    ),
                    (Some("-1.00"), None, Some(FinancialIssue::NegativeAmount)),
                ] {
                    let amount =
                        raw.map_or_else(String::new, |raw| format!("<Amt Ccy='EUR'>{raw}</Amt>"));
                    let charge = format!("<Chrgs>{amount}<CdtDbtInd>DBIT</CdtDbtInd></Chrgs>");
                    let payload = if level == "entry" {
                        charge
                    } else {
                        format!("<NtryDtls><TxDtls>{charge}</TxDtls></NtryDtls>")
                    };
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>{payload}</Ntry></{record}></{body}></Document>"
                    );
                    let (entry, _, diagnostics) = cash_matrix_observations(family, &xml);
                    let charges = if level == "entry" {
                        entry.charges.as_ref()
                    } else {
                        entry.details[0].charges.as_ref()
                    };
                    let amount = charges
                        .and_then(|charges| charges.records.first())
                        .map(|record| &record.amount);
                    assert_eq!(amount.and_then(|amount| amount.amount_raw.as_deref()), raw);
                    assert_eq!(amount.and_then(|amount| amount.ct), expected_ct);
                    let prefix = if level == "entry" {
                        format!("{record}[0]/Ntry[0]/Chrgs")
                    } else {
                        format!("{record}[0]/Ntry[0]/NtryDtls/TxDtls[0]/Chrgs")
                    };
                    let located: Vec<_> = diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.path == format!("{prefix}/Amt"))
                        .collect();
                    if let Some(expected) = expected_issue {
                        assert_eq!(located.len(), 1, "{family}/{level}/{raw:?}");
                        assert_eq!(located[0].issue, expected);
                    } else {
                        assert_eq!(located.len(), 0);
                    }
                }
            }
        }
    }

    #[test]
    fn cash_details_refuse_conflicting_transaction_amount_routes() {
        use sepa::camt::FinancialIssue;
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.053.001.08'><BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls><Amt Ccy='EUR'>1.00</Amt><AmtDtls><TxAmt><Amt Ccy='EUR'>2.00</Amt></TxAmt></AmtDtls></TxDtls></NtryDtls></Ntry></Stmt></BkToCstmrStmt></Document>";
        let document = sepa::parse_camt053(xml).unwrap();
        let detail = &document.statements[0].entries[0].details[0];
        assert_eq!(detail.signed_ct(), None);
        assert_eq!(
            detail
                .transaction_amount
                .as_ref()
                .unwrap()
                .amount_raw
                .as_deref(),
            Some("2.00")
        );
        let agreeing = sepa::parse_camt053(&xml.replace("EUR'>2.00", "EUR'>1.00")).unwrap();
        assert_eq!(
            agreeing.statements[0].entries[0].details[0].signed_ct(),
            Some(100)
        );
        assert_eq!(agreeing.financial_diagnostics().len(), 0);
        let different_currency =
            sepa::parse_camt053(&xml.replace("EUR'>2.00", "USD'>1.00")).unwrap();
        assert_eq!(
            different_currency.statements[0].entries[0].details[0].signed_ct(),
            None
        );
        let invalid = sepa::parse_camt053(&xml.replace("EUR'>2.00", "EUR'>0.001")).unwrap();
        assert_eq!(
            invalid.statements[0].entries[0].details[0].signed_ct(),
            None
        );
        assert!(
            invalid
                .financial_diagnostics()
                .iter()
                .any(
                    |diagnostic| diagnostic.issue == FinancialIssue::SubCentPrecision
                        && diagnostic.path.ends_with("/AmtDtls/TxAmt/Amt")
                )
        );
        let mut mutated = agreeing;
        mutated.statements[0].entries[0].details[0]
            .transaction_amount
            .as_mut()
            .unwrap()
            .currency = Some("USD".to_owned());
        assert_eq!(
            mutated.statements[0].entries[0].details[0].signed_ct(),
            None
        );
        assert!(
            document
                .financial_diagnostics()
                .iter()
                .any(
                    |diagnostic| diagnostic.issue == FinancialIssue::ConflictingAlternatives
                        && diagnostic.path.ends_with("/TxDtls[0]")
                )
        );
    }

    #[test]
    fn incomplete_nested_cash_amount_never_inherits_entry_money() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls><AmtDtls><TxAmt/></AmtDtls></TxDtls></NtryDtls></Ntry></{record}></{body}></Document>"
            );
            for direct in ["", "<Amt Ccy='EUR'>1.00</Amt>"] {
                let incomplete = xml.replace("<TxDtls>", &format!("<TxDtls>{direct}"));
                let (entry, _, diagnostics) = cash_matrix_observations(family, &incomplete);
                let detail = &entry.details[0];
                assert_eq!(detail.signed_ct(), None, "{family}/{direct}");
                assert!(detail.amount_source.is_some());
                let nested = detail.transaction_amount.as_ref().unwrap();
                assert_eq!(nested.amount_raw, None);
                assert_eq!(nested.ct, None);
                let path = format!("{record}[0]/Ntry[0]/NtryDtls/TxDtls[0]/AmtDtls/TxAmt/Amt");
                assert!(diagnostics.iter().any(|diagnostic| {
                    diagnostic.path == path
                        && diagnostic.issue == FinancialIssue::MissingAmount
                        && diagnostic.raw.is_none()
                }));
            }
            let absent = xml.replace("<AmtDtls><TxAmt/></AmtDtls>", "");
            let (entry, _, diagnostics) = cash_matrix_observations(family, &absent);
            assert_eq!(entry.details[0].signed_ct(), Some(100));
            assert_eq!(entry.details[0].amount_source, None);
            assert!(entry.details[0].transaction_amount.is_none());
            assert_eq!(diagnostics.len(), 0);
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn cash_financial_field_matrix_retains_every_magnitude_state() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            for field in [
                "balance",
                "entry",
                "detail",
                "nested",
                "entry_total",
                "entry_record",
                "detail_total",
                "detail_record",
            ] {
                if family == "camt.054" && field == "balance" {
                    continue;
                }
                for (raw, expected_ct, expected_issue) in [
                    (None, None, None),
                    (Some(""), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("bad"), None, Some(FinancialIssue::InvalidAmount)),
                    (Some("1.23"), Some(123), None),
                    (Some(" \n1.23\t "), Some(123), None),
                    (Some("0.001"), None, Some(FinancialIssue::SubCentPrecision)),
                    (
                        Some("999999999999999999999"),
                        None,
                        Some(FinancialIssue::Overflow),
                    ),
                    (Some("-1.00"), None, Some(FinancialIssue::NegativeAmount)),
                ] {
                    let tag = if field.ends_with("total") {
                        "TtlChrgsAndTaxAmt"
                    } else {
                        "Amt"
                    };
                    let amount = raw
                        .map_or_else(String::new, |raw| format!("<{tag} Ccy='EUR'>{raw}</{tag}>"));
                    let charge = if field.ends_with("record") {
                        format!("<Chrgs><Rcrd>{amount}<CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs>")
                    } else {
                        format!("<Chrgs>{amount}</Chrgs>")
                    };
                    let base_amount = "<Amt Ccy='EUR'>1.00</Amt>";
                    let entry_amount = if field == "entry" {
                        amount.as_str()
                    } else {
                        base_amount
                    };
                    let balance = if field == "balance" {
                        format!("<Bal>{amount}<CdtDbtInd>CRDT</CdtDbtInd></Bal>")
                    } else {
                        String::new()
                    };
                    let detail = match field {
                        "detail" => format!("<NtryDtls><TxDtls>{amount}</TxDtls></NtryDtls>"),
                        "nested" if raw.is_some() => format!(
                            "<NtryDtls><TxDtls><AmtDtls><TxAmt>{amount}</TxAmt></AmtDtls></TxDtls></NtryDtls>"
                        ),
                        "nested" => "<NtryDtls><TxDtls/></NtryDtls>".to_owned(),
                        "detail_total" | "detail_record" => {
                            format!("<NtryDtls><TxDtls>{charge}</TxDtls></NtryDtls>")
                        }
                        _ => String::new(),
                    };
                    let entry_charge = if field.starts_with("entry_") {
                        charge.as_str()
                    } else {
                        ""
                    };
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id>{balance}<Ntry>{entry_amount}<CdtDbtInd>CRDT</CdtDbtInd>{entry_charge}{detail}</Ntry></{record}></{body}></Document>"
                    );
                    let (entry, balances, diagnostics) = cash_matrix_observations(family, &xml);
                    let base = format!("{record}[0]/Ntry[0]");
                    let (observed_raw, observed_ct, path) = match field {
                        "balance" => (
                            balances[0].amount.amount_raw.clone(),
                            balances[0].amount.ct,
                            format!("{record}[0]/Bal[0]/Amt"),
                        ),
                        "entry" => (
                            entry.amount.amount_raw.clone(),
                            entry.amount.ct,
                            format!("{base}/Amt"),
                        ),
                        "detail" | "nested" => (
                            entry.details[0].amount.amount_raw.clone(),
                            entry.details[0].amount.ct,
                            format!(
                                "{base}/NtryDtls/TxDtls[0]/{}",
                                if field == "nested" {
                                    "AmtDtls/TxAmt/Amt"
                                } else {
                                    "Amt"
                                }
                            ),
                        ),
                        _ => {
                            let charges = if field.starts_with("entry_") {
                                entry.charges.as_ref()
                            } else {
                                entry.details[0].charges.as_ref()
                            };
                            let base = if field.starts_with("entry_") {
                                format!("{base}/Chrgs")
                            } else {
                                format!("{base}/NtryDtls/TxDtls[0]/Chrgs")
                            };
                            if field.ends_with("total") {
                                (
                                    charges.and_then(|charges| charges.total_amount_raw.clone()),
                                    charges.and_then(|charges| charges.total_ct),
                                    format!("{base}/TtlChrgsAndTaxAmt"),
                                )
                            } else {
                                let reported = &charges.unwrap().records[0].amount;
                                (
                                    reported.amount_raw.clone(),
                                    reported.ct,
                                    format!("{base}/Rcrd[0]/Amt"),
                                )
                            }
                        }
                    };
                    assert_eq!(observed_raw.as_deref(), raw, "{family}/{field}");
                    assert_eq!(observed_ct, expected_ct, "{family}/{field}/{raw:?}");
                    let expected_issue = expected_issue.or_else(|| {
                        (raw.is_none()
                            && matches!(
                                field,
                                "balance" | "entry" | "entry_record" | "detail_record"
                            ))
                        .then_some(FinancialIssue::MissingAmount)
                    });
                    let located: Vec<_> = diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.path == path)
                        .collect();
                    if let Some(expected) = expected_issue {
                        assert_eq!(located.len(), 1, "{family}/{field}/{raw:?}");
                        assert_eq!(located[0].issue, expected);
                        assert_eq!(located[0].raw.as_deref(), raw);
                    } else {
                        assert_eq!(located.len(), 0, "{family}/{field}/{raw:?}");
                    }
                }
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn cash_currency_and_direction_states_remain_observations() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            for field in [
                "balance",
                "entry",
                "detail",
                "nested",
                "entry_record",
                "detail_record",
                "entry_flat",
                "detail_flat",
            ] {
                if family == "camt.054" && field == "balance" {
                    continue;
                }
                for (currency, currency_issue) in [
                    (None, Some(FinancialIssue::MissingCurrency)),
                    (Some(""), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EU"), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EUR"), None),
                ] {
                    for (direction, expected_sign) in [
                        (None, None),
                        (Some(""), None),
                        (Some("dbit"), None),
                        (Some("DBIT"), Some(-123)),
                        (Some("CRDT"), Some(123)),
                    ] {
                        let attribute =
                            currency.map_or_else(String::new, |raw| format!(" Ccy='{raw}'"));
                        let amount = format!("<Amt{attribute}>1.23</Amt>");
                        let indicator = direction.map_or_else(String::new, |raw| {
                            format!("<CdtDbtInd>{raw}</CdtDbtInd>")
                        });
                        let charge = if field.ends_with("flat") {
                            format!("<Chrgs>{amount}{indicator}</Chrgs>")
                        } else {
                            format!("<Chrgs><Rcrd>{amount}{indicator}</Rcrd></Chrgs>")
                        };
                        let entry_amount = if field == "entry" {
                            amount.as_str()
                        } else {
                            "<Amt Ccy='EUR'>1.00</Amt>"
                        };
                        let entry_indicator = if field == "entry" || direction.is_none() {
                            indicator.as_str()
                        } else {
                            "<CdtDbtInd>CRDT</CdtDbtInd>"
                        };
                        let balance = if field == "balance" {
                            format!("<Bal>{amount}{indicator}</Bal>")
                        } else {
                            String::new()
                        };
                        let detail = match field {
                            "detail" => {
                                format!("<NtryDtls><TxDtls>{amount}{indicator}</TxDtls></NtryDtls>")
                            }
                            "nested" => format!(
                                "<NtryDtls><TxDtls><AmtDtls><TxAmt>{amount}</TxAmt></AmtDtls>{indicator}</TxDtls></NtryDtls>"
                            ),
                            "detail_record" | "detail_flat" => {
                                format!("<NtryDtls><TxDtls>{charge}</TxDtls></NtryDtls>")
                            }
                            _ => String::new(),
                        };
                        let entry_charge = if field.starts_with("entry_") {
                            charge.as_str()
                        } else {
                            ""
                        };
                        let xml = format!(
                            "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id>{balance}<Ntry>{entry_amount}{entry_indicator}{entry_charge}{detail}</Ntry></{record}></{body}></Document>"
                        );
                        let (entry, balances, diagnostics) = cash_matrix_observations(family, &xml);
                        let base = format!("{record}[0]/Ntry[0]");
                        let (observed, path) = match field {
                            "balance" => (&balances[0].amount, format!("{record}[0]/Bal[0]/Amt")),
                            "entry" => (&entry.amount, format!("{base}/Amt")),
                            "detail" => (
                                &entry.details[0].amount,
                                format!("{base}/NtryDtls/TxDtls[0]/Amt"),
                            ),
                            "nested" => (
                                entry.details[0].transaction_amount.as_ref().unwrap(),
                                format!("{base}/NtryDtls/TxDtls[0]/AmtDtls/TxAmt/Amt"),
                            ),
                            _ => {
                                let (charges, base) = if field.starts_with("entry_") {
                                    (entry.charges.as_ref().unwrap(), format!("{base}/Chrgs"))
                                } else {
                                    (
                                        entry.details[0].charges.as_ref().unwrap(),
                                        format!("{base}/NtryDtls/TxDtls[0]/Chrgs"),
                                    )
                                };
                                (
                                    &charges.records[0].amount,
                                    format!(
                                        "{base}/{}",
                                        if field.ends_with("flat") {
                                            "Amt"
                                        } else {
                                            "Rcrd[0]/Amt"
                                        }
                                    ),
                                )
                            }
                        };
                        assert_eq!(observed.amount_raw.as_deref(), Some("1.23"));
                        assert_eq!(observed.ct, Some(123));
                        assert_eq!(observed.currency.as_deref(), currency, "{family}/{field}");
                        assert_eq!(
                            observed.direction_raw.as_deref(),
                            direction,
                            "{family}/{field}"
                        );
                        assert_eq!(
                            observed.signed_ct(),
                            if currency_issue.is_none() {
                                expected_sign
                            } else {
                                None
                            },
                            "{family}/{field}/{currency:?}/{direction:?}"
                        );
                        let currency_path = format!("{path}/@Ccy");
                        let issues: Vec<_> = diagnostics
                            .iter()
                            .filter(|diagnostic| {
                                diagnostic.path == currency_path
                                    && matches!(
                                        diagnostic.issue,
                                        FinancialIssue::MissingCurrency
                                            | FinancialIssue::InvalidCurrency
                                    )
                            })
                            .collect();
                        assert_eq!(
                            issues.len(),
                            usize::from(currency_issue.is_some()),
                            "{family}/{field}/{currency:?}"
                        );
                        if let Some(issue) = currency_issue {
                            assert_eq!(issues[0].issue, issue);
                            assert_eq!(issues[0].raw.as_deref(), currency);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cash_aggregate_currencies_are_never_defaulted() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            for detail_level in [false, true] {
                for (currency, issue) in [
                    (None, Some(FinancialIssue::MissingCurrency)),
                    (Some(""), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EU"), Some(FinancialIssue::InvalidCurrency)),
                    (Some("EUR"), None),
                ] {
                    let attribute =
                        currency.map_or_else(String::new, |raw| format!(" Ccy='{raw}'"));
                    let charges = format!(
                        "<Chrgs><TtlChrgsAndTaxAmt{attribute}>1.23</TtlChrgsAndTaxAmt></Chrgs>"
                    );
                    let content = if detail_level {
                        format!("<NtryDtls><TxDtls>{charges}</TxDtls></NtryDtls>")
                    } else {
                        charges
                    };
                    let xml = format!(
                        "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd>{content}</Ntry></{record}></{body}></Document>"
                    );
                    let (entry, _, diagnostics) = cash_matrix_observations(family, &xml);
                    let charges = if detail_level {
                        entry.details[0].charges.as_ref().unwrap()
                    } else {
                        entry.charges.as_ref().unwrap()
                    };
                    assert_eq!(charges.total_amount_raw.as_deref(), Some("1.23"));
                    assert_eq!(charges.total_ct, Some(123));
                    assert_eq!(charges.total_currency.as_deref(), currency);
                    assert_eq!(charges.total_signed_ct(), None);
                    let base = format!("{record}[0]/Ntry[0]");
                    let base = if detail_level {
                        format!("{base}/NtryDtls/TxDtls[0]")
                    } else {
                        base
                    };
                    let path = format!("{base}/Chrgs/TtlChrgsAndTaxAmt/@Ccy");
                    let primary: Vec<_> = diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic.path == path
                                && matches!(
                                    diagnostic.issue,
                                    FinancialIssue::MissingCurrency
                                        | FinancialIssue::InvalidCurrency
                                )
                        })
                        .collect();
                    assert_eq!(primary.len(), usize::from(issue.is_some()));
                    if let Some(issue) = issue {
                        assert_eq!(primary[0].issue, issue);
                        assert_eq!(primary[0].raw.as_deref(), currency);
                    }
                }
            }
        }
    }

    fn cash_matrix_observations(
        family: &str,
        xml: &str,
    ) -> (
        sepa::camt::CashEntry,
        Vec<sepa::camt::StatementBalance>,
        Vec<sepa::camt::FinancialDiagnostic>,
    ) {
        match family {
            "camt.052" => {
                let document = sepa::parse_camt052(xml).unwrap();
                let diagnostics = document.financial_diagnostics();
                let report = document.reports.into_iter().next().unwrap();
                (
                    report.entries.into_iter().next().unwrap(),
                    report.balances,
                    diagnostics,
                )
            }
            "camt.053" => {
                let document = sepa::parse_camt053(xml).unwrap();
                let diagnostics = document.financial_diagnostics();
                let statement = document.statements.into_iter().next().unwrap();
                (
                    statement.entries.into_iter().next().unwrap(),
                    statement.balances,
                    diagnostics,
                )
            }
            _ => {
                let document = sepa::parse_camt054(xml).unwrap();
                let diagnostics = document.financial_diagnostics();
                let notification = document.notifications.into_iter().next().unwrap();
                (
                    notification.entries.into_iter().next().unwrap(),
                    Vec::new(),
                    diagnostics,
                )
            }
        }
    }

    #[test]
    fn cash_detail_diagnostics_locate_nested_transaction_amounts() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls><TxDtls><AmtDtls><TxAmt><Amt Ccy='EUR'>0.001</Amt></TxAmt></AmtDtls></TxDtls></NtryDtls></Ntry></{record}></{body}></Document>"
            );
            let diagnostics = match family {
                "camt.052" => sepa::parse_camt052(&xml).unwrap().financial_diagnostics(),
                "camt.053" => sepa::parse_camt053(&xml).unwrap().financial_diagnostics(),
                _ => sepa::parse_camt054(&xml).unwrap().financial_diagnostics(),
            };
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].issue, FinancialIssue::SubCentPrecision);
            assert_eq!(diagnostics[0].raw.as_deref(), Some("0.001"));
            assert_eq!(
                diagnostics[0].path,
                format!("{record}[0]/Ntry[0]/NtryDtls/TxDtls[0]/AmtDtls/TxAmt/Amt")
            );
        }
    }

    #[test]
    fn every_cash_reader_locates_unrepresentable_entries_and_charges() {
        use sepa::camt::FinancialIssue;
        for (family, body, record) in [
            ("camt.052", "BkToCstmrAcctRpt", "Rpt"),
            ("camt.053", "BkToCstmrStmt", "Stmt"),
            ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn"),
        ] {
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id><Ntry><Amt Ccy='EUR'>0.001</Amt><CdtDbtInd>DBIT</CdtDbtInd><Chrgs><TtlChrgsAndTaxAmt Ccy='EUR'>bad</TtlChrgsAndTaxAmt></Chrgs></Ntry></{record}></{body}></Document>"
            );
            let diagnostics = match family {
                "camt.052" => sepa::parse_camt052(&xml).unwrap().financial_diagnostics(),
                "camt.053" => sepa::parse_camt053(&xml).unwrap().financial_diagnostics(),
                _ => sepa::parse_camt054(&xml).unwrap().financial_diagnostics(),
            };
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.path
                == format!("{record}[0]/Ntry[0]/Amt")
                && diagnostic.issue == FinancialIssue::SubCentPrecision
                && diagnostic.raw.as_deref() == Some("0.001")));
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.path.ends_with("/Chrgs/TtlChrgsAndTaxAmt")
                    && diagnostic.issue == FinancialIssue::InvalidAmount
                    && diagnostic.raw.as_deref() == Some("bad")
            }));
        }
    }

    #[test]
    fn financial_diagnostics_distinguish_invalid_precision_overflow_and_missing() {
        use sepa::camt::{FinancialIssue, ReportedAmount};
        for (raw, issue) in [
            ("", FinancialIssue::InvalidAmount),
            ("x", FinancialIssue::InvalidAmount),
            ("0.001", FinancialIssue::SubCentPrecision),
            ("999999999999999999999", FinancialIssue::Overflow),
            ("-1.00", FinancialIssue::NegativeAmount),
        ] {
            let mut amount = ReportedAmount::default();
            amount.amount_raw = Some(raw.to_owned());
            let diagnostics = amount.diagnostics("Ntry[2]/Amt", Some("Ntry[2]/CdtDbtInd"));
            assert_eq!(diagnostics[0].issue, issue);
            assert_eq!(diagnostics[0].path, "Ntry[2]/Amt");
            assert_eq!(diagnostics[0].raw.as_deref(), Some(raw));
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.issue == FinancialIssue::MissingCurrency)
            );
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.issue == FinancialIssue::MissingDirection)
            );
        }
        let missing = ReportedAmount::default().diagnostics("Amt", None);
        assert_eq!(missing[0].issue, FinancialIssue::MissingAmount);
        assert_eq!(missing[0].raw, None);
        let mut exact = ReportedAmount::default();
        exact.amount_raw = Some(" 1.20 ".to_owned());
        exact.ct = Some(120);
        exact.currency = Some("EUR".to_owned());
        assert_eq!(exact.diagnostics("Amt", None).len(), 0);
        let mut inconsistent = exact;
        inconsistent.ct = Some(121);
        assert_eq!(
            inconsistent.diagnostics("Amt", None)[0].issue,
            FinancialIssue::InconsistentValue
        );
    }

    #[test]
    fn every_declared_reader_version_has_an_identity_positive() {
        for (family, versions, body) in [
            (
                "pain.002",
                &["001.03", "001.10", "002.03", "003.03"][..],
                "<CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId></OrgnlGrpInfAndSts></CstmrPmtStsRpt>",
            ),
            (
                "camt.029",
                &["001.06"][..],
                "<RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt></RsltnOfInvstgtn>",
            ),
            (
                "camt.052",
                &["001.02", "001.06", "001.08"][..],
                "<BkToCstmrAcctRpt><GrpHdr><MsgId>M</MsgId></GrpHdr></BkToCstmrAcctRpt>",
            ),
            (
                "camt.053",
                &["001.02", "001.06", "001.08"][..],
                "<BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr></BkToCstmrStmt>",
            ),
            (
                "camt.054",
                &["001.08"][..],
                "<BkToCstmrDbtCdtNtfctn><GrpHdr><MsgId>M</MsgId></GrpHdr></BkToCstmrDbtCdtNtfctn>",
            ),
        ] {
            for version in versions {
                let xml = format!(
                    "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.{version}'>{body}</Document>"
                );
                let accepted = match family {
                    "pain.002" => sepa::parse_pain002(&xml).is_ok(),
                    "camt.029" => sepa::parse_camt029(&xml).is_ok(),
                    "camt.052" => sepa::parse_camt052(&xml).is_ok(),
                    "camt.053" => sepa::parse_camt053(&xml).is_ok(),
                    _ => sepa::parse_camt054(&xml).is_ok(),
                };
                assert!(accepted, "declared {family}.{version} identity must parse");
            }
        }
    }

    #[test]
    fn changed_parent_currency_cannot_reconcile_a_cached_detail() {
        for detail in ["<TxDtls/>", "<TxDtls><Amt Ccy='EUR'>1.00</Amt></TxDtls>"] {
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.053.001.08'><BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id><Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><NtryDtls>{detail}</NtryDtls></Ntry></Stmt></BkToCstmrStmt></Document>"
            );
            let mut document = sepa::parse_camt053(&xml).unwrap();
            let entry = &mut document.statements[0].entries[0];
            assert!(entry.details_reconcile());
            entry.amount.currency = Some("USD".to_owned());
            assert_eq!(entry.details_signed_sum_ct(), None);
            assert!(!entry.details_reconcile());
        }
    }

    #[test]
    fn all_cash_totals_refuse_overflow_and_mixed_currencies() {
        let large =
            "<Ntry><Amt Ccy='EUR'>9000000000000000.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>"
                .repeat(11);
        for (entries, expected) in [
            (large.clone(), None),
            ("<Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry><Ntry><Amt Ccy='USD'>2.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>".to_owned(), None),
            ("<Ntry><Amt Ccy='eur'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry>".to_owned(), None),
            (format!("{large}{}", large.replace("CRDT", "DBIT")), Some(0)),
            ("<Ntry><Amt Ccy='EUR'>1.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Ntry><Ntry><Amt Ccy='EUR'>2.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Ntry>".to_owned(), Some(-100)),
        ] {
            for (family, body, record) in [("camt.052", "BkToCstmrAcctRpt", "Rpt"), ("camt.053", "BkToCstmrStmt", "Stmt"), ("camt.054", "BkToCstmrDbtCdtNtfctn", "Ntfctn")] {
                let xml = format!("<Document xmlns='urn:iso:std:iso:20022:tech:xsd:{family}.001.08'><{body}><GrpHdr><MsgId>M</MsgId></GrpHdr><{record}><Id>R</Id>{entries}</{record}></{body}></Document>");
                let total = match family {
                    "camt.052" => sepa::parse_camt052(&xml).unwrap().reports[0].net_movement_ct(),
                    "camt.053" => sepa::parse_camt053(&xml).unwrap().statements[0].net_movement_ct(),
                    _ => sepa::parse_camt054(&xml).unwrap().notifications[0].net_movement_ct(),
                };
                assert_eq!(total, expected, "{family} must produce only exact comparable totals");
            }
        }
    }

    #[test]
    fn missing_status_does_not_erase_a_reported_financial_bucket() {
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId><NbOfTxsPerSts><DtldNbOfTxs>3</DtldNbOfTxs><DtldCtrlSum>123.45</DtldCtrlSum></NbOfTxsPerSts></OrgnlGrpInfAndSts></CstmrPmtStsRpt></Document>";
        let document = sepa::parse_pain002(xml).unwrap();
        assert_eq!(document.group_status_counts.len(), 1);
        assert_eq!(document.group_status_counts[0].total_ct, Some(12_345));
    }

    #[test]
    fn status_readers_retain_unreadable_money_and_independent_choice_branches() {
        for raw in [
            "",
            "not-money",
            "0.001",
            "999999999999999999999",
            " \n1.20\t ",
            "-1.00",
        ] {
            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.029.001.06'><RsltnOfInvstgtn><Assgnmt><Id>A</Id></Assgnmt><CxlDtls><OrgnlPmtInfAndSts><TxInfAndSts><OrgnlInstdAmt>{raw}</OrgnlInstdAmt><OrgnlTxRef><Amt><InstdAmt Ccy='USD'>2.00</InstdAmt><EqvtAmt><Amt Ccy='GBP'>3.001</Amt><CcyOfTrf>CHF</CcyOfTrf></EqvtAmt></Amt></OrgnlTxRef></TxInfAndSts></OrgnlPmtInfAndSts></CxlDtls></RsltnOfInvstgtn></Document>"
            );
            let document = sepa::parse_camt029(&xml).unwrap();
            let transaction = &document.groups[0].payment_infos[0].transactions[0];
            assert_eq!(transaction.original_amount_raw.as_deref(), Some(raw));
            assert_eq!(transaction.original_currency, None);
            assert_eq!(
                transaction.original_amount_ct,
                sepa::ct_from_eur_str(raw).ok().filter(|value| *value >= 0)
            );
            let reference = transaction.referenced_amount.as_ref().unwrap();
            assert_eq!(reference.ct, Some(200));
            assert_eq!(reference.currency.as_deref(), Some("USD"));
            let equivalent = transaction.referenced_equivalent_amount.as_ref().unwrap();
            assert_eq!(equivalent.ct, None);
            assert_eq!(equivalent.amount_raw.as_deref(), Some("3.001"));
            assert_eq!(equivalent.currency.as_deref(), Some("GBP"));
            assert_eq!(
                transaction.referenced_transfer_currency.as_deref(),
                Some("CHF")
            );

            let xml = format!(
                "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:pain.002.001.10'><CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId><NbOfTxsPerSts><DtldSts>ACTC</DtldSts><DtldCtrlSum>{raw}</DtldCtrlSum></NbOfTxsPerSts></OrgnlGrpInfAndSts><OrgnlPmtInfAndSts><TxInfAndSts><OrgnlTxRef><Amt><InstdAmt>{raw}</InstdAmt><EqvtAmt><Amt Ccy='GBP'>3.001</Amt><CcyOfTrf>CHF</CcyOfTrf></EqvtAmt></Amt></OrgnlTxRef></TxInfAndSts></OrgnlPmtInfAndSts></CstmrPmtStsRpt></Document>"
            );
            let document = sepa::parse_pain002(&xml).unwrap();
            assert_eq!(
                document.group_status_counts[0].total_raw.as_deref(),
                Some(raw)
            );
            let transaction = &document.payment_info_statuses[0].transactions[0];
            assert_eq!(transaction.original_amount_raw.as_deref(), Some(raw));
            assert_eq!(transaction.original_currency, None);
            assert_eq!(
                transaction
                    .original_equivalent_amount
                    .as_ref()
                    .unwrap()
                    .amount_raw
                    .as_deref(),
                Some("3.001")
            );
            assert_eq!(
                transaction.original_transfer_currency.as_deref(),
                Some("CHF")
            );
        }
    }

    #[test]
    fn malformed_code_case_cannot_be_promoted_to_acceptance_or_direction() {
        let status = "acsc".parse::<sepa::PaymentStatus>().unwrap();
        assert!(!status.is_accepted());
        assert_eq!(status.as_code(), "acsc");
        assert!("crdt".parse::<sepa::CreditDebitIndicator>().is_err());
        assert!("ACSC".parse::<sepa::PaymentStatus>().unwrap().is_accepted());
        assert_eq!(
            "CRDT".parse::<sepa::CreditDebitIndicator>().unwrap(),
            sepa::CreditDebitIndicator::Credit
        );
    }

    #[test]
    fn all_readers_enforce_the_same_expanded_message_identity() {
        type Reader = fn(&str) -> Option<String>;
        let readers: &[(&str, &str, &str, Reader)] = &[
            (
                "camt.029.001.06",
                "RsltnOfInvstgtn",
                "<Assgnmt><Id>A</Id></Assgnmt>",
                |xml| {
                    sepa::parse_camt029(xml)
                        .ok()
                        .and_then(|document| document.namespace)
                },
            ),
            ("camt.052.001.08", "BkToCstmrAcctRpt", "", |xml| {
                sepa::parse_camt052(xml)
                    .ok()
                    .and_then(|document| document.namespace)
            }),
            ("camt.053.001.08", "BkToCstmrStmt", "", |xml| {
                sepa::parse_camt053(xml)
                    .ok()
                    .and_then(|document| document.namespace)
            }),
            ("camt.054.001.08", "BkToCstmrDbtCdtNtfctn", "", |xml| {
                sepa::parse_camt054(xml)
                    .ok()
                    .and_then(|document| document.namespace)
            }),
            (
                "pain.002.001.10",
                "CstmrPmtStsRpt",
                "<GrpHdr><MsgId>M</MsgId><CreDtTm>2026-10-12T00:00:00</CreDtTm></GrpHdr><OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><OrgnlMsgNmId>pain.001.001.09</OrgnlMsgNmId></OrgnlGrpInfAndSts>",
                |xml| {
                    sepa::parse_pain002(xml)
                        .ok()
                        .and_then(|document| document.namespace)
                },
            ),
        ];
        for &(version, body, inner, read) in readers {
            let uri = format!("urn:iso:std:iso:20022:tech:xsd:{version}");
            let plain = format!("<Document xmlns='{uri}'><{body}>{inner}</{body}></Document>");
            let prefixed = format!(
                "<p:Document xmlns:p='{uri}' xmlns='{uri}'><p:{body}>{inner}</p:{body}></p:Document>"
            );
            assert_eq!(read(&plain).as_deref(), Some(uri.as_str()));
            assert_eq!(read(&prefixed), read(&plain));
            for invalid in [
                format!("<Other xmlns='{uri}'><{body}>{inner}</{body}></Other>"),
                format!("<Document><{body}>{inner}</{body}></Document>"),
                format!("<Document xmlns='urn:wrong'><{body}>{inner}</{body}></Document>"),
                format!(
                    "<Document xmlns='{uri}'><{body} xmlns='urn:wrong'>{inner}</{body}></Document>"
                ),
                format!("<Document xmlns='{uri}'><{body}>{inner}</{body}><{body}/></Document>"),
                plain.replace(version, "camt.053.001.99"),
                format!("{plain}<Extra/>"),
                format!("{plain}<unfinished"),
            ] {
                assert!(read(&invalid).is_none(), "{version}: {invalid}");
            }
        }
    }

    #[test]
    fn finite_overrides_are_shared_by_every_public_reader() {
        let invalid = sepa::ParseLimits {
            nodes: 0,
            ..Default::default()
        };
        assert!(
            [
                sepa::parse_camt029_with_limits("<R/>", invalid).is_err(),
                sepa::parse_camt052_with_limits("<R/>", invalid).is_err(),
                sepa::parse_camt053_with_limits("<R/>", invalid).is_err(),
                sepa::parse_camt054_with_limits("<R/>", invalid).is_err(),
                sepa::parse_pain002_with_limits("<R/>", invalid).is_err(),
            ]
            .into_iter()
            .all(|refused| refused)
        );
        let xml = "<Document xmlns='urn:iso:std:iso:20022:tech:xsd:camt.053.001.08'><BkToCstmrStmt/></Document>";
        let mut limits = sepa::ParseLimits {
            nodes: 1,
            ..Default::default()
        };
        assert!(sepa::parse_camt053_with_limits(xml, limits).is_err());
        limits.nodes = 2;
        assert!(sepa::parse_camt053_with_limits(xml, limits).is_ok());
    }
}

/// Values that are legal per XML Schema and must therefore be accepted.
///
/// A parser stricter than the standard it names fails on valid input, at the
/// caller, with nothing the caller can do about it. This crate has shipped that
/// bug three times — `validate_bic`'s six-letter prefix (D42), `ct_from_eur_str`
/// rejecting a leading `+`, and `IsoDate` rejecting a timezone (D50, D51) — and
/// each time the symptom appeared somewhere else entirely.
mod lexical_space {
    use super::*;

    #[test]
    fn every_xs_decimal_an_amount_element_may_carry_is_accepted() {
        // `ActiveOrHistoricCurrencyAndAmount` is an `xs:decimal` restricted to
        // `fractionDigits="5"` and `totalDigits="18"`. The lexical space is
        // `(\+|-)?([0-9]+(\.[0-9]*)?|\.[0-9]+)`, and `whiteSpace="collapse"`
        // means padding is legal too.
        for legal in [
            "0",
            "0.00",
            "1",
            "100",
            "1.5",
            "1.50",
            "-1.50",
            "+1.50", // signs
            "+0",
            "-0",
            "0.0",
            ".5",
            "+.5",
            "-.5",
            "1.",
            "+1.",
            "-1.", // edge forms
            "1.500",
            "1.23000",
            "0.10000", // insignificant trailing zeros
            " 1.50 ",
            "\n1.50\n",
            "\t1.50\t",  // whiteSpace="collapse"
            "000001.50", // leading zeros are legal
        ] {
            assert!(
                ct_from_eur_str(legal).is_ok(),
                "{legal:?} is a legal xs:decimal and must be accepted, got {:?}",
                ct_from_eur_str(legal)
            );
        }
    }

    #[test]
    fn an_xs_decimal_below_one_cent_is_refused_explicitly_not_silently() {
        // The one legal-but-unrepresentable case. Refusing is correct — this
        // type counts whole cents — but it must be a *named* refusal, never a
        // truncation, because the digits carry money (D50).
        for legal_but_unrepresentable in ["0.001", "1.999", "1.23456", "0.00001"] {
            assert!(
                matches!(
                    ct_from_eur_str(legal_but_unrepresentable),
                    Err(AmountError::SubCentPrecision { .. })
                ),
                "{legal_but_unrepresentable:?} must be refused by name, got {:?}",
                ct_from_eur_str(legal_but_unrepresentable)
            );
        }
    }

    /// `xs:boolean` has **four** lexical forms, and a parser for a
    /// schema-valid document must accept all of them.
    ///
    /// `ChrgInclInd` is the only boolean this crate reads, and it is not a
    /// decorative one: it decides whether a ledger posts a charge or treats it
    /// as already inside the entry amount. Getting it wrong double-counts
    /// money, which is the same cost as the `CdtDbtInd` defect (D48).
    #[test]
    fn every_xs_boolean_a_charge_indicator_may_carry_is_accepted() {
        for (literal, expected) in [
            ("true", true),
            ("1", true),
            ("false", false),
            ("0", false),
            // whiteSpace="collapse" — padding is legal.
            (" true ", true),
            ("\n0\n", false),
        ] {
            let doc = parse_camt053(&statement_with_charge(literal)).unwrap();
            let rec = &doc.statements[0].entries[0]
                .charges
                .as_ref()
                .expect("charges parsed")
                .records[0];
            assert_eq!(
                rec.included_in_amount,
                Some(expected),
                "ChrgInclInd {literal:?} is a legal xs:boolean and must resolve"
            );
        }
    }

    /// And an *unreadable* one stays distinguishable from an absent one.
    ///
    /// Both leave `included_in_amount` at `None`, because neither determines
    /// the answer — but "the bank said nothing" and "the bank said `TRUE`" are
    /// different facts, and a library that merges them has destroyed the
    /// second before the caller sees it (D9, P6).
    #[test]
    fn an_unreadable_charge_indicator_is_kept_verbatim_not_merged_with_absent() {
        // `xs:boolean` is case-sensitive: `TRUE` is not one of its four forms.
        for illegible in ["TRUE", "yes", "True", "2", "-1", "0.0"] {
            let doc = parse_camt053(&statement_with_charge(illegible)).unwrap();
            let rec = &doc.statements[0].entries[0]
                .charges
                .as_ref()
                .expect("charges parsed")
                .records[0];
            assert_eq!(
                rec.included_in_amount, None,
                "{illegible:?} must not resolve to a boolean"
            );
            assert_eq!(
                rec.included_in_amount_raw.as_deref(),
                Some(illegible),
                "{illegible:?} must be kept verbatim"
            );
        }

        // Absent is the other case, and it reads differently.
        let doc = parse_camt053(&statement_without_charge_indicator()).unwrap();
        let rec = &doc.statements[0].entries[0]
            .charges
            .as_ref()
            .expect("charges parsed")
            .records[0];
        assert_eq!(rec.included_in_amount, None);
        assert_eq!(
            rec.included_in_amount_raw, None,
            "an absent ChrgInclInd leaves nothing behind"
        );

        let doc = parse_camt053(&statement_with_charge("")).unwrap();
        let rec = &doc.statements[0].entries[0]
            .charges
            .as_ref()
            .expect("charges parsed")
            .records[0];
        assert_eq!(rec.included_in_amount, None);
        assert_eq!(rec.included_in_amount_raw.as_deref(), Some(""));
    }

    fn statement_with_charge(indicator: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Ntry><Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
      <Chrgs><Rcrd><Amt Ccy="EUR">3.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
        <ChrgInclInd>{indicator}</ChrgInclInd></Rcrd></Chrgs>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#
        )
    }

    fn statement_without_charge_indicator() -> String {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Ntry><Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
      <Chrgs><Rcrd><Amt Ccy="EUR">3.00</Amt><CdtDbtInd>DBIT</CdtDbtInd></Rcrd></Chrgs>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#
            .to_owned()
    }

    #[test]
    fn every_xs_date_an_isodate_element_may_carry_is_accepted() {
        // `xs:date` is `'-'? yyyy '-' mm '-' dd zzzzzz?` — the timezone is
        // optional but **legal**, and ISO 20022 types `ISODate` as plain
        // `xs:date`. Rejecting `2026-07-20Z` made this crate unable to read a
        // schema-valid document (D51).
        for legal in [
            "2026-07-20",
            "2026-07-20Z",
            "2026-07-20+02:00",
            "2026-07-20-05:00",
            "2026-07-20+14:00", // the maximum offset XML Schema permits
            "2026-07-20-14:00",
            "2024-02-29", // a real leap day
        ] {
            let parsed = legal.parse::<IsoDate>();
            assert!(
                parsed.is_ok(),
                "{legal:?} is a legal xs:date and must be accepted, got {parsed:?}"
            );
            // The zone is dropped, never carried into the calendar day: the
            // value denotes that day, and every date this crate writes is a
            // bare `xs:date`.
            let expected = legal.get(..10).unwrap();
            assert_eq!(parsed.unwrap().to_string(), expected);
        }
    }

    #[test]
    fn an_offset_outside_the_xsd_range_is_not_a_timezone() {
        // ±14:00 is the limit, and minutes must be zero at the limit.
        for illegal in [
            "2026-07-20+15:00",
            "2026-07-20+14:01",
            "2026-07-20+2:00",
            "2026-07-20Q",
        ] {
            assert!(
                illegal.parse::<IsoDate>().is_err(),
                "{illegal:?} is not a legal xs:date timezone"
            );
        }
    }

    #[test]
    fn every_xs_datetime_an_isodatetime_element_may_carry_is_accepted() {
        // `xs:dateTime` permits arbitrary fractional seconds. Banks send them
        // in `CreDtTm`, and refusing would make a schema-valid document
        // unreadable. They are truncated to the second — every ISO 20022 field
        // this type serves is second-resolution — and the `*_raw` field beside
        // every parsed timestamp keeps the spelling that arrived.
        for legal in [
            "2026-07-20T09:00:00",
            "2026-07-20T09:00:00Z",
            "2026-07-20T09:00:00+02:00",
            "2026-07-20T09:00:00-05:00",
            "2026-07-20T09:00:00.0",
            "2026-07-20T09:00:00.123",
            "2026-07-20T09:00:00.123456789Z",
            "2026-07-20T00:00:00",
            "2026-07-20T23:59:59",
        ] {
            let parsed = legal.parse::<IsoDateTime>();
            assert!(
                parsed.is_ok(),
                "{legal:?} is a legal xs:dateTime and must be accepted, got {parsed:?}"
            );
            // Truncated to the second, never rounded, and the date and the
            // wall-clock time survive intact.
            let t = parsed.unwrap();
            assert_eq!(t.date().to_string(), "2026-07-20");
            assert_eq!(
                (t.hour(), t.minute(), t.second()),
                (
                    legal[11..13].parse().unwrap(),
                    legal[14..16].parse().unwrap(),
                    legal[17..19].parse().unwrap()
                ),
                "{legal:?}"
            );
        }
    }

    #[test]
    fn the_date_part_of_a_choice_accepts_exactly_its_two_members() {
        // `DateAndDateTimeChoice` holds an `xs:date` or an `xs:dateTime`. Those
        // two and nothing else: `parse_date_part` used to take the first ten
        // characters and ignore the rest, so `2026-07-20<anything>` became a
        // confident booking date (D51).
        for member in [
            "2026-07-20",
            "2026-07-20Z",
            "2026-07-20T12:30:00",
            "2026-07-20T12:30:00Z",
        ] {
            assert_eq!(
                IsoDate::parse_date_part(member).unwrap(),
                IsoDate::new(2026, 7, 20).unwrap(),
                "{member:?} is a member of the choice"
            );
        }
        for not_a_member in [
            "2026-07-20GARBAGE",
            "2026-07-20 12:30:00", // a space is not the `T` separator
            "2026-07-20T",
            "2026-07-20Tnonsense",
            "2026-07-20T99:99:99",
        ] {
            assert!(
                IsoDate::parse_date_part(not_a_member).is_err(),
                "{not_a_member:?} is not an xs:date or an xs:dateTime, got {:?}",
                IsoDate::parse_date_part(not_a_member)
            );
        }
    }

    #[test]
    fn the_amount_formatter_and_parser_agree_over_the_whole_i64_range() {
        // D44: a total function's inverse must be total over its image. The
        // edges are the point — `i64::MIN` has no positive counterpart.
        for v in [
            i64::MIN,
            i64::MIN + 1,
            -100_000,
            -1,
            0,
            1,
            99_999_999_999,
            i64::MAX - 1,
            i64::MAX,
        ] {
            let printed = ct_to_eur_str(v);
            assert_eq!(
                ct_from_eur_str(&printed),
                Ok(v),
                "{v} printed as {printed:?} must parse back"
            );
        }
    }
}

// ── the parser must not invent ────────────────────────────────────────────────

/// Every value reported must be derivable from the bytes that arrived.
///
/// This is the gate that would have caught the worst defect the crate has had.
/// `CdtDbtInd` was parsed with `.unwrap_or(Credit)`, so a debit whose indicator
/// a bank mistyped became a **credit of the same magnitude** — and no test saw
/// it, because all 25 `CdtDbtInd` occurrences in the whole fixture corpus were
/// a correctly spelled `CRDT` or `DBIT`. The invalid branch of a two-branch
/// enum had no coverage at all (D48).
mod no_fabrication {
    use super::*;

    /// A one-entry camt.053 with the given amount and indicator elements.
    fn statement(amount: &str, indicator: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Ntry><Amt Ccy="EUR">{amount}</Amt>{indicator}</Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#
        )
    }

    /// Near-misses for `CRDT` / `DBIT` that a real file can carry: a
    /// transposition, a truncation, a wrong code, an empty element, an absent
    /// one, and the spelled-out words a hand-built file uses.
    const NEAR_MISSES: [&str; 9] = [
        "<CdtDbtInd>DBTI</CdtDbtInd>",
        "<CdtDbtInd>CRTD</CdtDbtInd>",
        "<CdtDbtInd>DB</CdtDbtInd>",
        "<CdtDbtInd>DEBIT</CdtDbtInd>",
        "<CdtDbtInd>Debit</CdtDbtInd>",
        "<CdtDbtInd>-</CdtDbtInd>",
        "<CdtDbtInd></CdtDbtInd>",
        "<CdtDbtInd>  </CdtDbtInd>",
        "", // absent, though the schema makes it mandatory
    ];

    #[test]
    fn a_direction_is_never_reported_that_the_file_did_not_state() {
        for near_miss in NEAR_MISSES {
            let doc = parse_camt053(&statement("1000.00", near_miss)).unwrap();
            let entry = &doc.statements[0].entries[0];
            assert_eq!(
                entry.amount.direction, None,
                "{near_miss:?} must not resolve to a direction"
            );
            assert_eq!(
                entry.signed_ct(),
                None,
                "{near_miss:?} must not produce a ledger figure"
            );
            // The magnitude is still known — only the direction is not.
            assert_eq!(entry.amount.ct, Some(100_000), "{near_miss:?}");
        }
    }

    #[test]
    fn the_two_wire_codes_still_resolve() {
        // The gate above must not be satisfied by refusing everything.
        for (code, want) in [("CRDT", 100_000), ("DBIT", -100_000)] {
            let xml = statement("1000.00", &format!("<CdtDbtInd>{code}</CdtDbtInd>"));
            let doc = parse_camt053(&xml).unwrap();
            assert_eq!(
                doc.statements[0].entries[0].signed_ct(),
                Some(want),
                "{code:?} is a real code and must resolve"
            );
        }
    }

    #[test]
    fn a_currency_is_never_reported_that_the_file_did_not_state() {
        // `Amt/@Ccy` is a required attribute, so an absent one means the
        // document is already outside the schema — but defaulting it to `EUR`
        // is not a harmless guess. camt statements are not EUR-only, and the
        // fabricated currency *propagates*: a detail is excluded from its
        // entry's sum when the two currencies differ, so guessing here silently
        // changes which transactions are counted.
        let no_ccy = statement("10.00", "<CdtDbtInd>CRDT</CdtDbtInd>")
            .replace(r#"<Amt Ccy="EUR">"#, "<Amt>");
        let doc = parse_camt053(&no_ccy).unwrap();
        assert_eq!(doc.statements[0].entries[0].amount.currency, None);

        // …and a currency that *is* stated survives verbatim, whatever it is.
        for stated in ["EUR", "CHF", "JPY"] {
            let xml = statement("10.00", "<CdtDbtInd>CRDT</CdtDbtInd>")
                .replace("Ccy=\"EUR\"", &format!("Ccy=\"{stated}\""));
            let doc = parse_camt053(&xml).unwrap();
            assert_eq!(
                doc.statements[0].entries[0].amount.currency.as_deref(),
                Some(stated)
            );
        }
    }

    #[test]
    fn a_balance_with_no_type_is_not_a_balance_with_an_empty_type() {
        // `Tp` is mandatory on a `CashBalance`. An absent one used to become
        // `Other("")`, which is exactly what a bank sending an *empty* code
        // produces — two different facts collapsed into one value.
        let doc = parse_camt053(
            r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
<BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
<Bal><Amt Ccy="EUR">10.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Bal>
<Bal><Tp><CdOrPrtry><Cd></Cd></CdOrPrtry></Tp>
  <Amt Ccy="EUR">20.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Bal>
</Stmt></BkToCstmrStmt></Document>"#,
        )
        .unwrap();
        let balances = &doc.statements[0].balances;
        // An absent `Tp` and an empty `<Cd></Cd>` are both "no type stated" —
        // crate-wide, an element with no text carries no value. What matters is
        // that neither is `Other("")`, which is what a *code* of zero length
        // would be and is a different assertion about the file.
        assert_eq!(balances[0].balance_type, sepa::BalanceType::Unspecified);
        assert_eq!(balances[1].balance_type, sepa::BalanceType::Unspecified);

        // A code that is actually there round-trips.
        let doc = parse_camt053(
            r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
<BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
<Bal><Tp><CdOrPrtry><Cd>XPCD</Cd></CdOrPrtry></Tp>
  <Amt Ccy="EUR">20.00</Amt><CdtDbtInd>CRDT</CdtDbtInd></Bal>
</Stmt></BkToCstmrStmt></Document>"#,
        )
        .unwrap();
        assert_eq!(
            doc.statements[0].balances[0].balance_type,
            sepa::BalanceType::Other("XPCD".to_owned())
        );
    }

    #[test]
    fn a_status_bucket_survives_a_count_it_cannot_read() {
        // The bank asserting that a status bucket exists is information. A row
        // dropped for an unreadable count is indistinguishable from a bucket
        // the bank never mentioned (D49, one message over).
        // An empty element carries no value, so its raw is `None` too — the
        // same rule every other field here follows.
        for (raw, want, want_raw) in [
            ("3", Some(3), Some("3")),
            ("abc", None, Some("abc")),
            ("", None, Some("")),
            ("999999999999999999999", None, Some("999999999999999999999")),
        ] {
            let xml = format!(
                r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
<CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-01-01T00:00:00</CreDtTm></GrpHdr>
<OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><GrpSts>ACTC</GrpSts>
<NbOfTxsPerSts><DtldNbOfTxs>{raw}</DtldNbOfTxs><DtldSts>ACTC</DtldSts></NbOfTxsPerSts>
</OrgnlGrpInfAndSts></CstmrPmtStsRpt></Document>"#
            );
            let doc = sepa::parse_pain002(&xml).unwrap();
            assert_eq!(
                doc.group_status_counts.len(),
                1,
                "{raw:?}: row must survive"
            );
            assert_eq!(doc.group_status_counts[0].count, want, "{raw:?}");
            assert_eq!(
                doc.group_status_counts[0].count_raw.as_deref(),
                want_raw,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_resolved_figure_agrees_with_the_parts_it_was_resolved_from() {
        // Three structural invariants, over every combination of the cases
        // above. They are what makes "did not invent" checkable without
        // restating the parser.
        let amounts = ["1000.00", "0.00", "+1.50", "1.23456", "not-a-number", ""];
        let indicators = [
            "<CdtDbtInd>CRDT</CdtDbtInd>",
            "<CdtDbtInd>DBIT</CdtDbtInd>",
            "<CdtDbtInd>DBTI</CdtDbtInd>",
            "",
        ];
        for amount in amounts {
            for indicator in indicators {
                let doc = parse_camt053(&statement(amount, indicator)).unwrap();
                let e = &doc.statements[0].entries[0];

                // 1. A figure exists exactly when both of its parts do.
                assert_eq!(
                    e.signed_ct().is_some(),
                    e.amount.ct.is_some() && e.amount.direction.is_some(),
                    "{amount:?}/{indicator:?}: a ledger figure needs a magnitude AND a direction"
                );
                // 2. Signing changes the sign and nothing else.
                if let (Some(signed), Some(magnitude)) = (e.signed_ct(), e.amount.ct) {
                    assert_eq!(
                        signed.unsigned_abs(),
                        magnitude.unsigned_abs(),
                        "{amount:?}/{indicator:?}: signing must not change the magnitude"
                    );
                    assert_eq!(
                        signed < 0,
                        e.amount.direction == Some(CreditDebitIndicator::Debit) && magnitude != 0,
                        "{amount:?}/{indicator:?}: the sign must follow the indicator"
                    );
                }
                // 3. Whatever arrived is still there to look at. The `<Amt>`
                //    element is always present here, so its text is always
                //    retained — including when that text is empty, which is
                //    itself a fact about the file.
                assert_eq!(
                    e.amount.amount_raw.as_deref(),
                    Some(amount),
                    "{amount:?}: the text that arrived must survive verbatim"
                );
                assert_eq!(
                    e.amount.direction_raw.is_some(),
                    !indicator.is_empty(),
                    "{indicator:?}: the text that arrived must survive"
                );
            }
        }
    }
}

// ── the parser must not drop ──────────────────────────────────────────────────

/// Everything the input carried must be accounted for in the output.
///
/// The gate that would have caught D49. `parse_entry` returned `Option` and was
/// collected with `filter_map`, so an entry whose amount could not be read
/// **vanished from the statement** — and an integration test asserted exactly
/// that, having been written to pin down a panic fix rather than to ask whether
/// the resulting behaviour was right.
///
/// A missing booking is the one parse failure an importer cannot detect: it
/// looks identical to a booking that never happened. A wrong figure at least
/// fails a reconciliation.
mod no_silent_drop {
    use super::*;

    fn statement_with(entries: &[(&str, &str)]) -> String {
        let mut body = String::new();
        for (amt, ind) in entries {
            let _ = write!(body, "<Ntry><Amt Ccy=\"EUR\">{amt}</Amt>{ind}</Ntry>");
        }
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>{body}</Stmt></BkToCstmrStmt>
</Document>"#
        )
    }

    #[test]
    fn every_entry_element_produces_exactly_one_entry() {
        let hostile: Vec<(&str, &str)> = vec![
            ("100.00", "<CdtDbtInd>CRDT</CdtDbtInd>"),       // ordinary
            ("1.23456", "<CdtDbtInd>CRDT</CdtDbtInd>"),      // legal, unrepresentable
            ("+50.00", "<CdtDbtInd>DBIT</CdtDbtInd>"),       // legal leading sign
            ("not-a-number", "<CdtDbtInd>CRDT</CdtDbtInd>"), // malformed
            ("", "<CdtDbtInd>CRDT</CdtDbtInd>"),             // empty
            ("100.00", "<CdtDbtInd>DBTI</CdtDbtInd>"),       // unreadable direction
            ("100.00", ""),                                  // absent direction
            ("99999999999999999999999.00", "<CdtDbtInd>CRDT</CdtDbtInd>"), // past i64
        ];
        let doc = parse_camt053(&statement_with(&hostile)).unwrap();
        assert_eq!(
            doc.statements[0].entries.len(),
            hostile.len(),
            "every <Ntry> must be reported, readable or not — a dropped booking \
             is indistinguishable from one that never happened"
        );
    }

    #[test]
    fn a_total_refuses_rather_than_quietly_omitting_what_it_could_not_read() {
        // Skipping unresolved rows produces a number that looks right and is
        // not. `None` says "ask about this file"; a partial sum says nothing.
        let doc = parse_camt053(&statement_with(&[
            ("100.00", "<CdtDbtInd>CRDT</CdtDbtInd>"),
            ("50.00", "<CdtDbtInd>DBTI</CdtDbtInd>"),
        ]))
        .unwrap();
        assert_eq!(doc.statements[0].net_movement_ct(), None);

        // …and still answers when every row resolves.
        let doc = parse_camt053(&statement_with(&[
            ("100.00", "<CdtDbtInd>CRDT</CdtDbtInd>"),
            ("50.00", "<CdtDbtInd>DBIT</CdtDbtInd>"),
        ]))
        .unwrap();
        assert_eq!(doc.statements[0].net_movement_ct(), Some(5_000));
    }

    #[test]
    fn an_unreadable_entry_does_not_reconcile_by_default() {
        // `details_reconcile` compared against a fabricated entry total. With
        // no total to compare against there is nothing to agree with, and
        // "true" would be a guard that cannot fail.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Ntry><Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBTI</CdtDbtInd>
      <NtryDtls>
        <TxDtls><Amt Ccy="EUR">100.00</Amt></TxDtls>
        <TxDtls><Amt Ccy="EUR">25.00</Amt></TxDtls>
      </NtryDtls>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#;
        let doc = parse_camt053(xml).unwrap();
        assert!(!doc.statements[0].entries[0].details_reconcile());
    }
}

// ── the same rule at every level ──────────────────────────────────────────────

/// A rule applied to one type must hold for its neighbours.
///
/// A rule applied at one level of a structure and not the others is how
/// three money defects existed at once. `EntryDetail::signed_ct` returns
/// `Option<i64>` for exactly the right reason; `CashEntry::signed_ct`, forty
/// lines away in the same file, once manufactured a figure instead — and
/// nothing compared the two.
mod cross_level_consistency {
    use super::*;

    #[test]
    fn every_level_of_the_read_path_reports_money_the_same_way() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
    <Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp>
      <Amt Ccy="EUR">500.00</Amt><CdtDbtInd>DBTI</CdtDbtInd></Bal>
    <Ntry><Amt Ccy="EUR">125.00</Amt><CdtDbtInd>DBTI</CdtDbtInd>
      <Chrgs><Rcrd><Amt Ccy="EUR">3.00</Amt><CdtDbtInd>DBTI</CdtDbtInd></Rcrd></Chrgs>
      <NtryDtls><TxDtls><Amt Ccy="EUR">100.00</Amt>
        <CdtDbtInd>DBTI</CdtDbtInd></TxDtls></NtryDtls>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>"#;
        let doc = parse_camt053(xml).unwrap();
        let stmt = &doc.statements[0];
        let entry = &stmt.entries[0];

        // The same unreadable code, at four levels. Every one of them must
        // answer the same way: no figure, magnitude kept, raw retained.
        assert_eq!(stmt.balances[0].signed_ct(), None, "balance");
        assert_eq!(entry.signed_ct(), None, "entry");
        assert_eq!(entry.details[0].signed_ct(), None, "detail");
        assert_eq!(
            entry.charges.as_ref().unwrap().records[0].signed_ct(),
            None,
            "charge record"
        );

        assert_eq!(stmt.balances[0].amount.ct, Some(50_000));
        assert_eq!(entry.amount.ct, Some(12_500));
        assert_eq!(entry.details[0].amount.ct, Some(10_000));
        assert_eq!(
            entry.charges.as_ref().unwrap().records[0].amount.ct,
            Some(300)
        );

        for raw in [
            stmt.balances[0].amount.direction_raw.as_deref(),
            entry.amount.direction_raw.as_deref(),
            entry.details[0].amount.direction_raw.as_deref(),
            entry.charges.as_ref().unwrap().records[0]
                .amount
                .direction_raw
                .as_deref(),
        ] {
            assert_eq!(raw, Some("DBTI"), "every level keeps what arrived");
        }

        // And an all-or-nothing total never sums what it could not resolve.
        assert_eq!(entry.details_signed_sum_ct(), None);
        assert_eq!(entry.charges.as_ref().unwrap().total_signed_ct(), None);
        assert_eq!(stmt.net_movement_ct(), None);
    }

    #[test]
    fn every_document_type_keeps_its_creation_timestamp_verbatim_and_typed() {
        // Verbatim *and* typed, never in place of — at every level that has
        // a timestamp, not just the ones somebody remembered.
        let camt = parse_camt053(
            r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
<BkToCstmrStmt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-20T09:00:00.123Z</CreDtTm></GrpHdr>
<Stmt><Id>S</Id></Stmt></BkToCstmrStmt></Document>"#,
        )
        .unwrap();
        assert_eq!(camt.created_at_raw, "2026-07-20T09:00:00.123Z");
        assert_eq!(
            camt.created_at().map(|t| t.to_string()).as_deref(),
            Some("2026-07-20T09:00:00Z"),
            "fractional seconds are truncated in the typed value and kept in the raw"
        );

        let pain = sepa::parse_pain002(
            r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.001.10">
<CstmrPmtStsRpt><GrpHdr><MsgId>M</MsgId><CreDtTm>2026-07-20T09:00:00.123Z</CreDtTm></GrpHdr>
<OrgnlGrpInfAndSts><OrgnlMsgId>O</OrgnlMsgId><GrpSts>ACTC</GrpSts></OrgnlGrpInfAndSts>
</CstmrPmtStsRpt></Document>"#,
        )
        .unwrap();
        assert_eq!(pain.created_at_raw, "2026-07-20T09:00:00.123Z");
        assert_eq!(
            pain.created_at().map(|t| t.to_string()).as_deref(),
            Some("2026-07-20T09:00:00Z")
        );
    }
}

// ── regressions the seeded fuzzer found ───────────────────────────────────────

/// Found by `cargo fuzz run parse` once it was given a seed corpus.
///
/// Both are recorded here because a fuzzer finding is only a regression test if
/// somebody writes it down: the corpus is not checked in, and a cold run may
/// not rediscover them.
mod fuzzer_regressions {
    use super::*;

    #[test]
    fn a_malformed_timezone_is_rejected_not_a_panic() {
        // The timezone check computed `byte - b'0'` before verifying the byte
        // was a digit, so any byte below '0' underflowed and panicked — on
        // bank-supplied text, in a crate that lints against panics precisely
        // because it parses bank-supplied text.
        for hostile in [
            "2026-07-20+!!:!!",
            "2026-07-20+  :  ",
            "2026-07-20-\u{0}\u{0}:\u{0}\u{0}",
            "2026-07-20+AB:CD",
            "2026-07-20+--:--",
        ] {
            assert!(
                hostile.parse::<IsoDate>().is_err(),
                "{hostile:?} must be rejected"
            );
            assert!(IsoDate::parse_date_part(hostile).is_err());
        }
    }

    #[test]
    fn a_sole_detail_may_outlive_its_own_amount_but_never_its_direction() {
        // The one place the money invariant is deliberately weaker: a single
        // `TxDtls` with no itemised amount inherits the entry's, which is safe
        // because there is exactly one transaction to attribute it to. It must
        // still never acquire a *direction* the file did not state.
        let sole = |indicator: &str| {
            format!(
                r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
<BkToCstmrStmt><GrpHdr><MsgId>M</MsgId></GrpHdr><Stmt><Id>S</Id>
<Ntry><Amt Ccy="EUR">75.00</Amt>{indicator}
<NtryDtls><TxDtls><RmtInf><Ustrd>x</Ustrd></RmtInf></TxDtls></NtryDtls>
</Ntry></Stmt></BkToCstmrStmt></Document>"#
            )
        };

        // Inherits the entry's amount *and* direction.
        let doc = parse_camt053(&sole("<CdtDbtInd>DBIT</CdtDbtInd>")).unwrap();
        let detail = &doc.statements[0].entries[0].details[0];
        assert_eq!(detail.amount.ct, None, "the detail itemised no amount");
        assert_eq!(
            detail.signed_ct(),
            Some(-7_500),
            "but the entry's is attributable"
        );

        // The entry's direction is unreadable, so the detail has none either.
        let doc = parse_camt053(&sole("<CdtDbtInd>DBTI</CdtDbtInd>")).unwrap();
        let detail = &doc.statements[0].entries[0].details[0];
        assert_eq!(detail.amount.direction, None);
        assert_eq!(detail.signed_ct(), None, "no direction, no figure");
    }
}
