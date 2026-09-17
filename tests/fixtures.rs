//! Every inline ISO 20022 fixture in `src/` meets a real schema.
//!
//! A fixture no schema has seen proves only that the parser agrees with
//! whoever wrote it. `tests/integration.rs` gates the fixtures it *shares*
//! with the unit tests; this covers the rest — every document literal in
//! `src/`, validated against the schema its own namespace names.
//!
//! The walk is deliberate rather than a list: hoisting each fixture into a
//! shared set by hand would cover the ones that exist today and none written
//! next month.
//!
//! ## Scope
//!
//! Doc-comment examples are **not** covered. They are minimal on purpose and
//! are already executed as doctests. What is covered is every literal in
//! ordinary code and in `#[cfg(test)]` modules — the fixtures that decide
//! what the parsers are believed to do.
//!
//! A fixture whose namespace names an unvendored schema is **reported on
//! every run**, never silently skipped. There are currently none.
//!
//! ## Opting out
//!
//! Some fixtures are deliberately not valid documents — they prove the parser
//! rejects something. Mark those on one of the three lines above the literal:
//!
//! ```text
//! // xsd-exempt: a camt.053 stub handed to the camt.029 parser on purpose
//! ```
//!
//! The reason is required, so an exemption is a decision somebody wrote down
//! rather than a fixture that quietly stopped being checked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A document literal found in the source.
struct Fixture {
    file: String,
    line: usize,
    namespace: String,
    xml: String,
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Pull every `r#"..."#` literal that looks like a whole ISO 20022 document.
fn fixtures_in(path: &Path) -> Vec<Fixture> {
    let src = std::fs::read_to_string(path).expect("source file is readable");
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_owned();
    let lines: Vec<&str> = src.lines().collect();

    let mut out = Vec::new();
    let mut rest = src.as_str();
    let mut consumed = 0usize;

    while let Some(open) = rest.find("r#\"") {
        let body_start = open + 3;
        let Some(close) = rest[body_start..].find("\"#") else {
            break;
        };
        let body = &rest[body_start..body_start + close];
        let abs = consumed + open;
        let line = src[..abs].matches('\n').count() + 1;

        consumed = abs + 3 + close + 2;
        rest = &src[consumed..];

        // Doc-comment examples are deliberately out of scope. They are
        // minimal on purpose — a rustdoc snippet that carried a full valid
        // `GrpHdr` to satisfy a schema would bury the one thing it is there to
        // show — and they are already executed as doctests, so they cannot
        // drift from the API. R11's residual is about fixtures that shape
        // *parser behaviour* inside `#[cfg(test)]`, which is what D43 was.
        let opening = lines.get(line.saturating_sub(1)).copied().unwrap_or("");
        let trimmed = opening.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("//!") {
            continue;
        }

        // Only whole documents: a fragment has no namespace to pick a schema
        // from, and a `format!` template is not yet a document.
        let Some(ns) = document_namespace(body) else {
            continue;
        };
        if body.contains('{') || body.contains('}') {
            continue;
        }
        // An exemption may sit on any of the three lines above the literal.
        let exempt = line
            .checked_sub(4)
            .map(|from| &lines[from..line.saturating_sub(1).min(lines.len())])
            .is_some_and(|window| window.iter().any(|l| l.contains("xsd-exempt:")));
        if exempt {
            continue;
        }
        out.push(Fixture {
            file: file.clone(),
            line,
            namespace: ns,
            xml: body.to_owned(),
        });
    }
    out
}

/// `urn:iso:std:iso:20022:tech:xsd:camt.053.001.08` → that namespace, but only
/// when it is on a `<Document` root.
fn document_namespace(body: &str) -> Option<String> {
    let doc = body.find("<Document")?;
    let after = &body[doc..];
    let key = "urn:iso:std:iso:20022:tech:xsd:";
    let start = after.find(key)? + key.len();
    let tail = &after[start..];
    let end = tail.find(['"', '\''])?;
    Some(tail[..end].to_owned())
}

fn xmllint(xml: &str, schema: &Path) -> Option<Result<(), String>> {
    let dir = std::env::temp_dir().join(format!("sepa-fixture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let doc = dir.join("doc.xml");
    std::fs::write(&doc, xml).ok()?;
    let out = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(schema)
        .arg(&doc)
        .output()
        .ok()?;
    let _ = std::fs::remove_file(&doc);
    if out.status.success() {
        Some(Ok(()))
    } else {
        Some(Err(String::from_utf8_lossy(&out.stderr).into_owned()))
    }
}

/// **The gate.** Every inline document in `src/` validates against the schema
/// its own namespace names.
#[test]
fn every_inline_fixture_is_a_document_a_bank_could_have_sent() {
    let root = manifest_dir();
    let xsd_dir = root.join("tests/xsd");

    let mut sources: Vec<PathBuf> = std::fs::read_dir(root.join("src"))
        .expect("src/ is readable")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    sources.sort();

    let mut checked = 0usize;
    let mut skipped_no_schema: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut lint_missing = false;

    for path in &sources {
        for f in fixtures_in(path) {
            let schema = xsd_dir.join(format!("{}.xsd", f.namespace));
            if !schema.exists() {
                skipped_no_schema.push(format!("{}:{} → {}", f.file, f.line, f.namespace));
                continue;
            }
            match xmllint(&f.xml, &schema) {
                Some(Ok(())) => checked += 1,
                Some(Err(e)) => failures.push(format!(
                    "{}:{} against {}.xsd\n{}",
                    f.file,
                    f.line,
                    f.namespace,
                    e.trim()
                )),
                None => {
                    lint_missing = true;
                }
            }
        }
    }

    if lint_missing {
        eprintln!("SKIP: xmllint unavailable — inline fixtures not schema-checked");
        return;
    }

    // A schema this repository has not vendored cannot gate anything, and a
    // silent skip reads exactly like a passing check. Say so, loudly, every
    // run.
    if !skipped_no_schema.is_empty() {
        eprintln!(
            "NOTE: {} inline fixture(s) have no vendored schema and were not checked:\n  {}",
            skipped_no_schema.len(),
            skipped_no_schema.join("\n  ")
        );
    }

    assert!(
        failures.is_empty(),
        "\n{} inline fixture(s) are not documents any bank could send.\n\n{}\n\n\
         A fixture no schema has seen proves only that the parser agrees with \
         whoever wrote it (D43). Fix the fixture, or mark it \
         `// xsd-exempt: <reason>` if it is deliberately invalid.\n",
        failures.len(),
        failures.join("\n\n")
    );

    // The walk is only evidence if it is finding things. Two thresholds,
    // because they fail for different reasons: the first catches the
    // extractor breaking, the second catches the vendored schemas going away.
    let found = checked + skipped_no_schema.len();
    assert!(
        found >= 18,
        "the source walk found only {found} document literals — the extractor \
         has stopped working"
    );
    assert!(
        checked >= 12,
        "only {checked} of {found} fixtures were schema-checked — the vendored \
         schemas have gone missing"
    );
}
