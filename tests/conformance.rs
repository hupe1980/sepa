//! Conformance gates for the classes of defect the rest of the suite cannot see.
//!
//! Every other test here asks *would this output be accepted?* — the XSD gates,
//! the GBIC subsets, the field rules. Three whole classes of defect are
//! invisible to that question, and in September 2026 an audit found live
//! examples of all three. This file is the answer to "why did the tests not
//! catch them".
//!
//! | Class | Why the existing suite could not see it | Gate here |
//! |---|---|---|
//! | The crate **refuses** input a bank may legally send | A refusal produces no document to validate, so there is nothing for a schema gate to look at | [`lexical_space`] |
//! | The crate **invents** a value the input did not carry | A fabricated value is a perfectly ordinary `Ok`; the fuzzer asserted only "does not panic" | [`no_fabrication`] |
//! | The crate **drops** something the input did carry | The output is a valid, smaller document — and one test asserted the dropping as correct | [`no_silent_drop`] |
//!
//! The oracle in the first case is the **standard's own lexical space** rather
//! than a schema: XML Schema says exactly which strings are an `xs:decimal`, an
//! `xs:date` and an `xs:dateTime`, and a parser for a schema-valid document has
//! to accept all of them. In the second and third it is the **input itself** —
//! every value reported must be derivable from the bytes that came in, and
//! every element that came in must be accounted for.

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

        // An *empty* element reads as absent, and that is a crate-wide
        // convention rather than a property of this field: `Node::text_of`
        // returns `None` for an element with no text, so `<CdtDbtInd/>` and
        // `<ChrgInclInd/>` behave alike. Pinned here because it is the one
        // case where "the bank sent the element" and "the bank sent nothing"
        // are deliberately merged — an empty element is schema-invalid and
        // carries no value to preserve.
        let doc = parse_camt053(&statement_with_charge("")).unwrap();
        let rec = &doc.statements[0].entries[0]
            .charges
            .as_ref()
            .expect("charges parsed")
            .records[0];
        assert_eq!(rec.included_in_amount, None);
        assert_eq!(rec.included_in_amount_raw, None);
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
    fn the_two_codes_that_do_exist_still_resolve_in_both_cases() {
        // The gate above must not be satisfied by refusing everything.
        for (code, want) in [
            ("CRDT", 100_000),
            ("DBIT", -100_000),
            ("crdt", 100_000),
            ("dbit", -100_000),
        ] {
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
            ("", None, None),
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
