+++
title = "Postal addresses"
description = "Structured and hybrid PstlAdr postal addresses for SEPA payments in Rust — the two forms the schemes accept, whatever the migration date turns out to be."
weight = 9
+++

An address is optional in the SEPA schemes and most domestic batches carry
none. When one *is* present, its form is on a clock.

## The 2026 deadline

ISO 20022 allows three address forms. The European Payments Council is retiring
one of them:

| Form | Shape | Status |
|---|---|---|
| **Structured** | Dedicated elements only — street, building number, post code, town, country | Preferred |
| **Hybrid** | Town and country in their own elements, plus up to two free-text lines | Permitted |
| **Unstructured** | Free-text lines only, nothing machine-readable | Being retired — no end-date in force |

## The migration date

There is currently **no end-date in force** for unstructured addresses. The EPC
set 22 November 2026, moved it to 15 November 2026, then withdrew that on
9 September 2026 with a replacement due. A lot of published guidance still
quotes one of the old dates.

Migrate anyway — the direction has never changed, only the deadline. And build
against the rule rather than the date: an address is optional, but town and
country are mandatory **whenever one is present**. That has held throughout.

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

This is the same approach the crate takes to IBANs and dates: rather than
accepting a value and reporting the problem later, the invalid state has no
representation.

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
country subdivision, country and address lines. One address value therefore
validates against every schema version the crate emits.

At most two address lines are permitted; a third is an error rather than being
silently dropped. Lengths are checked *after* transliteration, because
`Straße` grows a character on its way to `Strasse`.

The legacy German schemas have no structured address type at all — theirs holds
only a country and two free-text lines — so selecting one and then setting an
address is rejected rather than emitting something that schema forbids.

## If all you have is free text

Then you have no address to build here. Omit it — the element is optional in
every SEPA schema. Do not invent a town to get past the constructor: it reaches
the bank as though your counterparty had supplied it.
