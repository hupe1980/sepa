//! Maintainer source-review, consumer-pin and vendored-integrity controls.
//!
//! [`WATCH`] records publication identities and review dates; [`CONSUMERS`]
//! records downstream version pins. Freshness is not proof that a review was
//! complete or that no newer publication exists.
//!
//! Time-dependent overdue checks run through `just watch` and the existing CI
//! watch job. This file is excluded from the consumer package.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use sepa::IsoDate;

#[test]
#[cfg(unix)]
fn vendored_integrity_refuses_changed_missing_and_unrecorded_bytes() {
    #[derive(Debug)]
    struct TemporaryCopy(std::path::PathBuf);
    impl Drop for TemporaryCopy {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = TemporaryCopy(
        std::env::temp_dir().join(format!("sepa-integrity-{}-{nonce}", std::process::id())),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let schemas = temporary.0.join("tests/xsd");
    let scripts = temporary.0.join("scripts");
    std::fs::create_dir_all(&schemas).unwrap();
    std::fs::create_dir_all(&scripts).unwrap();
    for entry in std::fs::read_dir(root.join("tests/xsd")).unwrap() {
        let entry = entry.unwrap();
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "xsd")
            || entry.file_name() == "README.md"
        {
            std::fs::copy(entry.path(), schemas.join(entry.file_name())).unwrap();
        }
    }
    let script = scripts.join("check-vendored-data.sh");
    std::fs::copy(root.join("scripts/check-vendored-data.sh"), &script).unwrap();
    let run = || {
        std::process::Command::new("bash")
            .arg(&script)
            .output()
            .unwrap()
    };
    let good = run();
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    let target = schemas.join("pain.001.001.09.xsd");
    let original = std::fs::read(&target).unwrap();
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n");
    std::fs::write(&target, changed).unwrap();
    let altered = run();
    assert!(!altered.status.success());
    assert!(String::from_utf8_lossy(&altered.stderr).contains("MISMATCH pain.001.001.09.xsd"));
    std::fs::remove_file(&target).unwrap();
    let missing = run();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("MISSING  pain.001.001.09.xsd"));
    std::fs::write(&target, original).unwrap();
    std::fs::write(schemas.join("pain.999.001.01.xsd"), b"unreviewed bytes").unwrap();
    let unrecorded = run();
    assert!(!unrecorded.status.success());
    assert!(String::from_utf8_lossy(&unrecorded.stderr).contains("UNRECORDED pain.999.001.01.xsd"));
}

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
        watch_for: "EPC153-22 revisions or an explicitly published replacement address end-date",
        url: "https://www.europeanpaymentscouncil.eu/document-library/guidance-documents",
        pinned: "EPC153-22 v2.2, 30 Sep 2026; withdrawn end-date has no replacement",
        last_verified: "2026-10-12",
        review_every_days: 30,
        reaches: &["src/address.rs", "site/content/docs/addresses.md"],
    },
    Source {
        id: "epc-2027-rulebooks",
        watch_for: "2028 rulebooks expected Q2 2027; future name limits require applicable C2PSP guidance",
        url: "https://www.europeanpaymentscouncil.eu/document-library/rulebooks",
        pinned: "2025 SCT rulebook v1.2, 30 Sep 2026; current C2PSP names remain 70 characters",
        last_verified: "2026-10-12",
        review_every_days: 30,
        reaches: &["src/validate.rs", "src/party.rs", "src/pain002.rs"],
    },
    Source {
        id: "epc-cr6-iso-migration",
        watch_for: "2030 ISO migration; technical guidance expected Q2 2029 and target ISO edition selection",
        url: "https://www.europeanpaymentscouncil.eu/what-we-do/epc-payment-scheme-management/evolution-schemes",
        pinned: "EPC 2030 migration planning; target ISO edition not fixed by the reviewed evidence",
        last_verified: "2026-10-12",
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
        watch_for: "EPC VoP inter-PSP revisions; DK pain-code authority is a separate C2PSP profile",
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
