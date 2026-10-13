+++
title = "Postal addresses"
description = "The library's structured and hybrid postal-address scope and applicable EPC guidance."
weight = 9
+++

Address presence depends on scheme, participants and legal case. Schema
optionality alone does not permit omission of required information.

## Current EPC Guidance

EPC153-22 v2.2, dated 30 September 2026, describes three address forms:

| Form | Shape | Status |
|---|---|---|
| **Structured** | Dedicated elements only — street, building number, post code, town, country | Preferred |
| **Hybrid** | Town and country in their own elements, plus up to two free-text lines | Permitted |
| **Unstructured** | Free-text address lines | Still permitted; no replacement cutoff set |

No unstructured-address replacement cutoff is set by this guidance.
Town/country are required for structured/hybrid, not every EPC-permitted format.

[Primary EPC guidance](https://www.europeanpaymentscouncil.eu/document-library/guidance-documents/epc-guidance-document-provision-addresses-under-epc-payment).

## Unstructured addresses are unrepresentable

`PostalAddress::new` takes the town and the country, so the free-text-only form
cannot be built:

```rust
use sepa::PostalAddress;
use sepa::address::AddressFormat;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Structured — dedicated elements, nothing free-text.
    let structured = PostalAddress::new("Berlin", "DE")?
        .street("Unter den Linden")
        .building_number("77")
        .post_code("10117");
    assert_eq!(structured.format(), AddressFormat::Structured);

    // Hybrid — town and country, plus what does not fit.
    let hybrid = PostalAddress::new("Amsterdam", "NL")?.line("Herengracht 1");
    assert_eq!(hybrid.format(), AddressFormat::Hybrid);

    // The country has to be a country. The schema's own pattern would take this.
    assert!(PostalAddress::new("Atlantis", "ZZ").is_err());
    Ok(())
}
```

Serde restoration uses the same town/country checks. Optional lengths, line count
and charset are checked at message validation, not all at initial construction.

## Attaching an address

The account holder's address belongs to the payment group; the counterparty's
belongs to the individual transaction.

```rust
use sepa::{CreditTransferEntry, CreditTransferGroup, IsoDate, PostalAddress, validate_iban};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let debtor = validate_iban("DE89370400440532013000")?;
    let creditor = validate_iban("NL91ABNA0417164300")?;
    let group = CreditTransferGroup::new("Acme GmbH", &debtor, IsoDate::new(2026, 7, 20)?)
        .debtor_address(PostalAddress::new("Berlin", "DE")?.street("Unter den Linden"))
        .add_entry(
            CreditTransferEntry::new("Supplier BV", creditor, 12_000, "E2E-1")
                .with_creditor_address(PostalAddress::new("Amsterdam", "NL")?),
        );

    assert_eq!(group.entry_count(), 1);
    Ok(())
}
```

Direct debits mirror this: `DirectDebitGroup::creditor_address` and
`DirectDebitEntry::with_debtor_address`.

## Which elements

Only the elements common to both generations of the ISO address type are
exposed — department, sub-department, street, building number, post code, town,
country subdivision, country and address lines. Message validation checks these
fields against the selected schema and charset policy.

At most two address lines are permitted; a third is an error rather than being
silently dropped. Lengths are checked *after* transliteration, because
`Straße` grows a character on its way to `Strasse`.

The legacy German schemas have no structured address type at all — theirs holds
only a country and two free-text lines — so selecting one and then setting an
address is rejected rather than emitting something that schema forbids.

## If all you have is free text

It is outside this library's emitted scope. Obtain required structured information
or use an implementation supporting the applicable permitted format. Omit only
when the applicable obligations permit it; never invent a town to pass construction.
