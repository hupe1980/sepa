//! The watch list, as a gate rather than as a note.
//!
//! Every other check here measures the crate against an artefact **inside**
//! the repository, which cannot notice that its publisher issued a newer one
//! — or that a regulator withdrew a deadline the crate's prose asserts. Two
//! defects have reached a release through that gap with the whole suite green.
//!
//! [`WATCH`] pins, per external source, the artefact in force and the date
//! somebody last read it. When `last_verified + review_every` falls behind
//! today the gate fails, naming the source, what to look for and the files a
//! change would reach. [`CONSUMERS`] does the same for the downstream
//! workspaces' pinned versions, checked against their manifests where present.
//!
//! The table is a Rust `const` rather than a parsed data file, so its shape is
//! checked by the compiler and no parser stands between the record and the
//! gate.
//!
//! ## Why the overdue check is `#[ignore]`d
//!
//! An overdue *reading* means a human must open a website — a different signal
//! from a broken build, and not a reason to fail an unrelated pull request.
//! It runs as `just watch` and as a CI job of its own. The file is excluded
//! from the published package for the same reason: a consumer must not inherit
//! a maintainer's review calendar.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use sepa::IsoDate;

/// One external publication this crate's correctness depends on.
#[derive(Debug)]
struct Source {
    /// Stable id, cited from `concepts/REGULATION.md`.
    id: &'static str,
    /// What to go and look for.
    watch_for: &'static str,
    /// Where to look.
    url: &'static str,
    /// The artefact version pinned by the crate right now.
    pinned: &'static str,
    /// When somebody last read the source. ISO 8601.
    last_verified: &'static str,
    /// How long that reading stays good, in days.
    review_every_days: i64,
    /// Paths a change would reach. Checked to exist, so a renamed module
    /// invalidates the row rather than silently outliving it.
    reaches: &'static [&'static str],
}

/// The sources, and how long a reading of each stays good.
///
/// The cadences differ because the failure modes do. A missing IBAN country
/// rejects a valid IBAN loudly, at the caller. A *changed* BBAN structure
/// accepts an IBAN that is now malformed, and the bank finds out. A withdrawn
/// deadline breaks nothing at all — which is why the EPC rows are the shortest
/// and the only ones whose trigger is a meeting rather than a document.
const WATCH: &[Source] = &[
    Source {
        id: "epc-news",
        watch_for: "deadline moves, rulebook errata, PSMB meeting outcomes",
        url: "https://www.europeanpaymentscouncil.eu/news-insights/news",
        pinned: "PSMB 9 Sep 2026: unstructured-address end-date withdrawn, no replacement",
        last_verified: "2026-09-17",
        review_every_days: 30,
        reaches: &["concepts/REGULATION.md", "src/address.rs"],
    },
    Source {
        id: "epc-address-end-date",
        watch_for: "EPC153-22 v2.2 carrying a new unstructured-address end-date",
        url: "https://www.europeanpaymentscouncil.eu/document-library/guidance-documents",
        pinned: "v2.1 (Oct 2025); no end-date in force",
        last_verified: "2026-09-17",
        review_every_days: 30,
        reaches: &["src/address.rs", "site/content/docs/addresses.md"],
    },
    Source {
        id: "epc-2027-rulebooks",
        watch_for: "the November 2026 publication — CR 4 (name 70→140), 17, 18, 22, 24, 36",
        url: "https://www.europeanpaymentscouncil.eu/document-library/rulebooks",
        pinned: "2025 rulebooks v1.1, in force since 5 Oct 2025",
        last_verified: "2026-09-17",
        review_every_days: 30,
        reaches: &["src/validate.rs", "src/party.rs", "src/pain002.rs"],
    },
    Source {
        id: "epc-cr6-iso-migration",
        watch_for: "whether the 2029 rulebooks migrate to a newer ISO 20022 version, and to which",
        url: "https://www.europeanpaymentscouncil.eu/what-we-do/epc-payment-scheme-management/evolution-schemes",
        pinned: "recommended for Nov 2029, likely the 2027 ISO version; EPC decision due Sep 2026, none published",
        last_verified: "2026-09-17",
        review_every_days: 30,
        reaches: &[
            "src/pain001.rs",
            "src/pain008.rs",
            "concepts/ARCHITECTURE.md",
        ],
    },
    Source {
        id: "epc-oct-inst",
        watch_for: "EPC250-22 / EPC158-22 revisions — the C2PSP rules this crate now emits",
        url: "https://www.europeanpaymentscouncil.eu/what-we-do/epc-payment-schemes/one-leg-out-instant-credit-transfer",
        pinned: "EPC250-22 2025 v1.0, effective 5 Oct 2025",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["src/pain001.rs", "src/currency.rs"],
    },
    Source {
        id: "epc-vop",
        watch_for: "VoP v2.0 (EPC084-26), especially a customer-to-PSP pain.001/pain.002 pair",
        url: "https://www.europeanpaymentscouncil.eu/what-we-do/epc-payment-schemes/verification-payee",
        pinned: "EPC218-23 v1.1, effective 20 Sep 2026; AT-R001 carries four outcomes",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["src/pain002.rs"],
    },
    Source {
        id: "swift-iban-registry",
        watch_for: "a release past r102 — a changed BBAN structure is the quiet failure",
        url: "https://www.swift.com/swift_resource/9606",
        pinned: "r102 (June 2026), 89 structures, 78 examples checked in CI",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["src/iban.rs"],
    },
    Source {
        id: "swift-standards-mx",
        watch_for: "the SR2027 scope — the likely basis of the 2029 migration, due by Dec 2026",
        url: "https://www.swift.com/standards/standards-releases",
        pinned: "SR2026; SR2027 scope not yet published",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["concepts/ARCHITECTURE.md"],
    },
    Source {
        id: "epc-sepa-countries",
        watch_for: "a version past EPC409-09 v8.0",
        url: "https://www.europeanpaymentscouncil.eu/document-library/other/epc-list-sepa-scheme-countries",
        pinned: "v8.0 (24 Dec 2025), 42 entries",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["src/iban.rs"],
    },
    Source {
        id: "iso-external-code-lists",
        watch_for: "Purp / CtgyPurp revisions",
        url: "https://www.iso20022.org/catalogue-messages/additional-content-messages/external-code-sets",
        pinned: "degrades gracefully: unknown codes survive in Purpose::Other",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["src/purpose.rs"],
    },
    Source {
        id: "dk-gbic",
        watch_for: "a GBIC 6 package, or Anlage 3 revisions — the strictest gate the crate has",
        url: "https://www.ebics.de/de/datenformate",
        pinned: "GBIC 5 subsets for pain.001.001.09 and pain.008.001.08",
        last_verified: "2026-09-17",
        review_every_days: 90,
        reaches: &["tests/xsd/pain.001.001.09_GBIC_5.xsd"],
    },
];

fn parse(date: &str, id: &str) -> IsoDate {
    IsoDate::parse(date)
        .unwrap_or_else(|e| panic!("{id}: last_verified {date:?} is not a date: {e}"))
}

/// Ids are unique and every `reaches` path still exists.
///
/// The second half is what keeps a row honest: a watch entry pointing at a
/// module that was renamed or deleted is describing a dependency the crate no
/// longer has, and would go on passing forever.
#[test]
fn every_row_is_well_formed_and_points_somewhere_real() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut ids: Vec<&str> = WATCH.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    let mut deduped = ids.clone();
    deduped.dedup();
    assert_eq!(ids, deduped, "watch ids must be unique");

    for s in WATCH {
        assert!(!s.watch_for.is_empty(), "{}: watch_for is empty", s.id);
        assert!(s.url.starts_with("https://"), "{}: url must be https", s.id);
        assert!(!s.pinned.is_empty(), "{}: nothing pinned", s.id);
        assert!(
            s.review_every_days > 0 && s.review_every_days <= 366,
            "{}: implausible cadence {}",
            s.id,
            s.review_every_days
        );
        assert!(!s.reaches.is_empty(), "{}: reaches nothing", s.id);
        for path in s.reaches {
            // `concepts/` is gitignored, so it may legitimately be absent in a
            // fresh clone; everything else must exist.
            if path.starts_with("concepts/") {
                continue;
            }
            assert!(
                root.join(path).exists(),
                "{}: reaches {path:?}, which does not exist — the row is stale",
                s.id
            );
        }
    }
}

/// No reading is dated in the future.
///
/// A future date would silence the gate indefinitely, which is the one way to
/// defeat it by accident.
#[test]
fn no_reading_is_dated_in_the_future() {
    let today = IsoDate::today();
    for s in WATCH {
        let seen = parse(s.last_verified, s.id);
        assert!(
            seen <= today,
            "{}: last_verified {} is in the future (today is {today})",
            s.id,
            s.last_verified
        );
    }
}

/// **The gate.** Fails when any source is overdue for a re-read.
///
/// This is expected to fail eventually — that is what it is for. Re-read the
/// source, update `pinned` if it moved, and set `last_verified` to today.
///
/// `#[ignore]` because it is the one test here whose result depends on the
/// date. An overdue review means *a human must go and read something*, which
/// is not a reason to block an unrelated pull request, and a contributor's
/// local `cargo test` should be deterministic. CI runs it as a job of its own
/// (`--include-ignored`) on every push and weekly, so it is loud without being
/// in the way: `just watch` is the same check.
#[test]
#[ignore = "time-dependent: run via `just watch` or the CI watch job"]
fn no_source_is_overdue_for_review() {
    let today = IsoDate::today();
    let mut overdue = Vec::new();

    for s in WATCH {
        let seen = parse(s.last_verified, s.id);
        let due = match seen.plus_days(s.review_every_days) {
            Ok(d) => d,
            // A cadence that runs off the end of the calendar is a bad row,
            // not a passing one.
            Err(e) => panic!(
                "{}: {} + {} days: {e}",
                s.id, s.last_verified, s.review_every_days
            ),
        };
        if due < today {
            overdue.push(format!(
                "  {}\n      due:       {due} (last read {}, every {} days)\n      \
                 watch for: {}\n      at:        {}\n      pinned:    {}\n      \
                 reaches:   {}",
                s.id,
                s.last_verified,
                s.review_every_days,
                s.watch_for,
                s.url,
                s.pinned,
                s.reaches.join(", ")
            ));
        }
    }

    assert!(
        overdue.is_empty(),
        "\n{} external source(s) overdue for review as of {today}.\n\n{}\n\n\
         This gate is the answer to the one thing every other check here cannot \
         see: a publisher issuing a new artefact, or a regulator withdrawing a \
         deadline this crate asserts. Re-read each source above, update `pinned` \
         if it moved, and set `last_verified` to today in tests/watch.rs.\n",
        overdue.len(),
        overdue.join("\n\n")
    );
}

// ── Consumers ─────────────────────────────────────────────────────────────

/// A downstream workspace and the version of this crate it pins.
#[derive(Debug)]
struct Consumer {
    workspace: &'static str,
    manifest: &'static str,
    pinned: &'static str,
    last_verified: &'static str,
    review_every_days: i64,
}

/// What each consumer pins, and when somebody last looked.
///
/// Kept here rather than in prose for the same reason as [`WATCH`]: the last
/// audit found this record wrong in three of its four rows, because a version
/// number written in a document nothing builds is a version number nobody
/// rechecks.
const CONSUMERS: &[Consumer] = &[
    Consumer {
        workspace: "mako",
        manifest: "../mako/Cargo.toml",
        pinned: "0.7",
        last_verified: "2026-09-17",
        review_every_days: 90,
    },
    Consumer {
        workspace: "emob",
        manifest: "../emob/Cargo.toml",
        pinned: "0.7",
        last_verified: "2026-09-17",
        review_every_days: 90,
    },
    Consumer {
        workspace: "esales",
        manifest: "../esales/Cargo.toml",
        pinned: "0.6",
        last_verified: "2026-09-17",
        review_every_days: 90,
    },
    Consumer {
        workspace: "en16931",
        manifest: "../en16931/crates/en16931/Cargo.toml",
        pinned: "0.5",
        last_verified: "2026-09-17",
        review_every_days: 90,
    },
];

/// Each consumer still pins what this table says.
///
/// Checked against the sibling manifest when it is present, which it is in a
/// full workspace checkout and is not in CI. Absent, the row still ages out
/// through [`no_source_is_overdue_for_review`].
#[test]
fn every_consumer_pins_what_this_table_records() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut wrong = Vec::new();
    let mut seen = 0usize;

    for c in CONSUMERS {
        let path = root.join(c.manifest);
        let Ok(manifest) = std::fs::read_to_string(&path) else {
            continue;
        };
        seen += 1;
        let found = manifest
            .lines()
            .filter(|l| l.trim_start().starts_with("sepa"))
            .find_map(|l| l.split("version").nth(1))
            .and_then(|v| v.split('"').nth(1).map(str::to_owned));
        match found {
            Some(v) if v == c.pinned => {}
            other => wrong.push(format!(
                "  {}: table says {:?}, {} says {:?}",
                c.workspace, c.pinned, c.manifest, other
            )),
        }
    }

    if seen == 0 {
        eprintln!("SKIP: no sibling workspaces checked out — consumer pins not verified");
        return;
    }
    assert!(
        wrong.is_empty(),
        "\n{} consumer pin(s) have moved since this table was written:\n{}\n",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Consumer readings age out on the same cadence as everything else.
#[test]
#[ignore = "time-dependent: run via `just watch` or the CI watch job"]
fn no_consumer_reading_is_overdue() {
    let today = IsoDate::today();
    let mut overdue = Vec::new();
    for c in CONSUMERS {
        let seen = parse(c.last_verified, c.workspace);
        let due = seen
            .plus_days(c.review_every_days)
            .unwrap_or_else(|e| panic!("{}: {e}", c.workspace));
        if due < today {
            overdue.push(format!(
                "  {} (pins {}, last read {}, due {due})",
                c.workspace, c.pinned, c.last_verified
            ));
        }
    }
    assert!(
        overdue.is_empty(),
        "\n{} consumer pin(s) overdue for a re-read as of {today}:\n{}\n\n\
         Read each workspace's Cargo.toml and update CONSUMERS in \
         tests/watch.rs.\n",
        overdue.len(),
        overdue.join("\n")
    );
}
