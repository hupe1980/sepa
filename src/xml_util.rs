//! Zero-allocation XML *writing* helpers for the ISO 20022 builders.
//!
//! Reading is handled by [`crate::xml`], which wraps [`quick_xml`]. Writing stays
//! hand-rolled: the builders emit a fixed, known-good element sequence, so a
//! generic writer would add indirection without buying correctness.

// ── zero-alloc XML write helpers ──────────────────────────────────────────────

/// Write `s` into `w` with XML character escaping — no heap allocation.
///
/// Scans for `&`, `<`, `>`, `"`, `'` and emits the corresponding entity between
/// runs of unescaped bytes, so typical SEPA names ("Max Mustermann `GmbH`") produce
/// a single `write_str` call with no branching overhead.
pub(crate) fn write_escaped<W: std::fmt::Write>(w: &mut W, s: &str) -> std::fmt::Result {
    let mut last = 0usize;
    for (i, b) in s.bytes().enumerate() {
        let entity = match b {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            b'\'' => "&apos;",
            _ => continue,
        };
        if last < i {
            w.write_str(&s[last..i])?;
        }
        w.write_str(entity)?;
        last = i + 1;
    }
    if last < s.len() {
        w.write_str(&s[last..])?;
    }
    Ok(())
}

/// Write `ct` (1/100 EUR) as `"1234.56"` directly into `w` — no String allocation.
pub(crate) fn write_eur<W: std::fmt::Write>(w: &mut W, ct: i64) -> std::fmt::Result {
    let sign = if ct < 0 { "-" } else { "" };
    let abs = ct.unsigned_abs();
    write!(w, "{sign}{}.{:02}", abs / 100, abs % 100)
}

/// Bridge `fmt::Write` → `io::Write`, preserving the original `io::Error`.
///
/// When the underlying write fails the `fmt::Error` sentinel is returned from
/// `write_str` and the real `io::Error` is stored in `error` for the caller to
/// retrieve.
pub(crate) struct IoWriterBridge<'a, W> {
    pub(crate) inner: &'a mut W,
    pub(crate) error: Option<std::io::Error>,
}

impl<W: std::io::Write> std::fmt::Write for IoWriterBridge<'_, W> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        match self.inner.write_all(s.as_bytes()) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.error = Some(e);
                Err(std::fmt::Error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── write_escaped ─────────────────────────────────────────────────────────

    #[test]
    fn escaped_no_special_chars() {
        let mut out = String::new();
        write_escaped(&mut out, "Max Mustermann GmbH").unwrap();
        assert_eq!(out, "Max Mustermann GmbH");
    }

    #[test]
    fn escaped_all_entities() {
        let mut out = String::new();
        write_escaped(&mut out, r#"A & B < C > D " E ' F"#).unwrap();
        assert_eq!(out, "A &amp; B &lt; C &gt; D &quot; E &apos; F");
    }

    #[test]
    fn escaped_only_ampersand() {
        let mut out = String::new();
        write_escaped(&mut out, "AT&T").unwrap();
        assert_eq!(out, "AT&amp;T");
    }

    #[test]
    fn escaped_empty() {
        let mut out = String::new();
        write_escaped(&mut out, "").unwrap();
        assert_eq!(out, "");
    }

    // ── write_eur ─────────────────────────────────────────────────────────────

    #[test]
    fn write_eur_positive() {
        let mut out = String::new();
        write_eur(&mut out, 7500).unwrap();
        assert_eq!(out, "75.00");
    }

    #[test]
    fn write_eur_negative() {
        let mut out = String::new();
        write_eur(&mut out, -500).unwrap();
        assert_eq!(out, "-5.00");
    }

    #[test]
    fn write_eur_zero() {
        let mut out = String::new();
        write_eur(&mut out, 0).unwrap();
        assert_eq!(out, "0.00");
    }
}
