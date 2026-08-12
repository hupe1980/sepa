# Specification downloads (untracked)

Staging area for specification material pulled from two sources:

- **[iso20022.org](https://www.iso20022.org)** — the ISO message definitions.
- **[ebics.de](https://www.ebics.de/de/datenformate/ergaenzende-dokumente)** —
  the Deutsche Kreditwirtschaft's *Anlage 3* package: GBIC technical validation
  subsets (TVS), the ISO originals it bundles, and published example files.
  Free, no registration.

Everything here is gitignored except this file.

## Download the **archive**, not the current catalogue

<https://www.iso20022.org/iso-20022-message-definitions> serves only the newest
version of each message — today `pain.001.001.13`, `pain.008.001.12`,
`pain.002.001.15`, `pain.007.001.13`. **None of those are SEPA versions.**

The site advises using the most recent definition "to ensure worldwide
coherence". That guidance is for communities free to choose; a SEPA participant
is not one, because the rulebook fixes the version. SEPA needs:

| Message | Version |
|---|---|
| pain.001 | `pain.001.001.09` |
| pain.008 | `pain.008.001.08` |
| pain.002 | `pain.002.001.10` |
| pain.007 | `pain.007.001.09` |

Those live in the **ISO 20022 Message Archive**:

<https://www.iso20022.org/catalogue-messages/iso-20022-messages-archive?search=pain>

Pick the *payments initiation* maintenance release that contains
`pain.001.001.09` — the same release carries the other three, so one download
covers every message this crate handles.

## Adding a schema to the test suite

1. Unpack the archive here.
2. Copy the single `<message-id>.xsd` into `tests/xsd/` — the file name must
   equal the variant's `message_id()`, e.g. `pain.007.001.09.xsd`, because
   `tests/integration.rs` derives the path from it.
3. Record its SHA-256 and provenance in `tests/xsd/README.md`.
4. Verify the choice types really are `xs:choice` before trusting it — see the
   mirror warning in that README. A widely-copied `pain.008.001.08.xsd` has them
   flattened to `xs:sequence` and rejects a valid `<SvcLvl><Cd>SEPA</Cd>`.
   A copy straight from the ISO archive is authoritative and needs no such
   corroboration.
