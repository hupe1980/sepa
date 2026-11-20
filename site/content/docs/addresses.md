+++
title = "Postal addresses"
description = "The library's structured and hybrid postal-address scope and applicable EPC guidance."
weight = 9
+++

Address presence depends on scheme, participants and legal case. Schema
optionality alone does not permit omission of required information.

## EPC guidance

EPC153-22 v2.2, dated 30 September 2026, describes three address forms:

| Form | Shape | Status |
|---|---|---|
| **Structured** | Dedicated elements only — street, building number, post code, town, country | Preferred |
| **Hybrid** | Town and country in their own elements, plus up to two free-text lines | Permitted |
| **Unstructured** | Free-text address lines | Permitted; the guidance sets no cutoff |

Town and country are required for structured and hybrid addresses.

[Primary EPC guidance](https://www.europeanpaymentscouncil.eu/document-library/guidance-documents/epc-guidance-document-provision-addresses-under-epc-payment).

## Structured and hybrid only

`PostalAddress::new` takes the town and the country, so the unstructured form
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

Serde restoration applies the same town/country checks. Lengths, line count and
charset are checked when the message is validated.

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

The elements common to both ISO address-type generations are exposed:
department, sub-department, street, building number, post code, town, country
subdivision, country and address lines.

At most two address lines are permitted; a third is an error. Lengths are
checked *after* transliteration (`Straße` → `Strasse`).

The legacy German schemas have no structured address type, so setting an address
with one of them selected is rejected.

## If all you have is free text

Unstructured addresses are out of scope. Obtain the structured information, or
use a tool that emits the unstructured form. Omit the address only where your
obligations permit; never invent a town to pass construction.
