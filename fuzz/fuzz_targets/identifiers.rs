//! Identifier validators and the charset conversion must never panic.
//!
//! These take user- and bank-supplied strings, and several index into them by
//! byte offset internally.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    let _ = sepa::validate_iban(s);
    let _ = sepa::validate_bic(s);
    let _ = sepa::validate_creditor_id(s);
    let _ = sepa::creditor_id_check_digits(s, "DE");
    let _ = sepa::ct_from_eur_str(s);
    let _ = s.parse::<sepa::IsoDate>();
    let _ = s.parse::<sepa::IsoDateTime>();
    // Takes the first ten *bytes* of bank-supplied text, so a multi-byte
    // character straddling the boundary must not panic.
    let _ = sepa::IsoDate::parse_date_part(s);
    let _ = sepa::is_country_code(s);
    let _ = sepa::iban_bban_format(s);
    let _ = sepa::iban_country_length(s);
    let _ = s.parse::<sepa::RfReference>();
    let _ = sepa::RfReference::generate(s);
    let _ = sepa::is_sepa_country(s);

    // Transliteration must always yield SEPA-legal output, for any input.
    for style in [sepa::Transliteration::German, sepa::Transliteration::Epc] {
        let out = sepa::transliterate(s, style);
        assert!(
            sepa::is_sepa_text(&out),
            "transliteration leaked a non-SEPA character: {out:?}"
        );
    }

    // Accessors on a successfully parsed identifier must not panic.
    if let Ok(iban) = sepa::validate_iban(s) {
        let _ = (iban.country_code(), iban.check_digits(), iban.bban());
        let _ = iban.is_sepa();
        let _ = iban.to_string();
        // Anything that validates must satisfy the registered structure, so a
        // structural check that disagrees with the validator is a bug.
        if let Some(structure) = sepa::iban_bban_format(iban.country_code()) {
            assert_eq!(structure.len(), iban.bban().len());
            assert!(
                sepa::validate_bic(&format!("AAAA{}FF", iban.country_code())).is_ok(),
                "an IBAN country must be an acceptable BIC country",
            );
        }
    }
    if let Ok(bic) = sepa::validate_bic(s) {
        let _ = (bic.institution_code(), bic.country_code(), bic.location_code());
        let _ = (bic.branch_code(), bic.is_test(), bic.is_passive());
    }

    // A parsed date must render back to something that parses identically, and
    // its calendar arithmetic must saturate rather than wrap or panic.
    if let Ok(date) = s.parse::<sepa::IsoDate>() {
        assert_eq!(sepa::IsoDate::parse(&date.to_string()), Ok(date));
        assert_eq!(
            sepa::IsoDate::from_epoch_days(date.epoch_days()),
            Ok(date),
            "epoch-day round trip must be lossless"
        );
        let offset = i64::try_from(data.len()).unwrap_or(i64::MAX) - 1_000;
        let _ = date.plus_days(offset);
    }
    if let Ok(ts) = s.parse::<sepa::IsoDateTime>() {
        assert_eq!(sepa::IsoDateTime::parse(&ts.to_string()), Ok(ts));
    }

    // A constructed address must always render to SEPA-legal XML, and its
    // validation must decide rather than panic on arbitrary text.
    if let Ok(address) = sepa::PostalAddress::new(s, "DE") {
        assert_eq!(address.country(), "DE");
        let _ = address.format();
        let _ = address
            .clone()
            .street(s)
            .post_code(s)
            .line(s)
            .validate(sepa::CharsetPolicy::default());
    }
    // Only a real country may be accepted, whatever the input looks like.
    assert_eq!(
        sepa::PostalAddress::new("Berlin", s).is_ok(),
        sepa::is_country_code(s),
        "address country acceptance must match the country table",
    );
});
