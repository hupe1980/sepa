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
    }
    if let Ok(bic) = sepa::validate_bic(s) {
        let _ = (bic.institution_code(), bic.country_code(), bic.location_code());
        let _ = (bic.branch_code(), bic.is_test(), bic.is_passive());
    }
});
