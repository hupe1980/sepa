//! Minimal XML text-extraction utilities for SEPA ISO 20022 message parsing.
//!
//! Design principles:
//! - **Namespace-agnostic**: call [`normalize_ns`] first to strip `<ns2:Tag>`
//!   prefixes so all searches work on bare element names
//! - **Minimal allocations**: one `String` per call for the close-tag sentinel
//! - Opening tags may carry attributes: `<InstdAmt Ccy="EUR">100.00</InstdAmt>`
//! - Nesting depth is tracked properly in `xml_inner` and `xml_each`
//!
//! ## Namespace handling
//!
//! Banks may send either:
//! - Default-namespace documents: `<Document xmlns="urn:..."><TxSts>…`  
//! - Prefixed documents: `<ns2:Document xmlns:ns2="urn:…"><ns2:TxSts>…`
//!
//! Call [`normalize_ns`] once on the raw XML string before passing to any
//! extraction function; this collapses both forms to bare element names.

// ── namespace normalization ───────────────────────────────────────────────────

/// Strip XML namespace prefixes from element names, producing bare-name XML.
///
/// | Input | Output |
/// |---|---|
/// | `<ns2:TxSts>RJCT</ns2:TxSts>` | `<TxSts>RJCT</TxSts>` |
/// | `<InstdAmt Ccy="EUR">100.00</InstdAmt>` | unchanged |
/// | `xmlns:ns2="urn:…"` attribute value | unchanged |
/// | `<?xml version="1.0"?>` declaration | unchanged |
/// | `<!-- comment -->` | unchanged |
///
/// This is a linear scan (O(n)) with a single output allocation.
pub(crate) fn normalize_ns(xml: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum S {
        Text,
        TagOpen,     // after `<`
        TagName,     // collecting element name
        Attr,        // inside tag, outside attribute value
        AttrVal(u8), // inside attribute value, delimiter stored
        Special,     // after `<!` or `<?` — pass through until `>`
    }

    let mut out = String::with_capacity(xml.len());
    let mut state = S::Text;
    let mut tag_buf = String::new();

    for c in xml.chars() {
        match state {
            S::Text => {
                if c == '<' {
                    out.push(c);
                    state = S::TagOpen;
                } else {
                    out.push(c);
                }
            }
            S::TagOpen => match c {
                '/' => {
                    out.push(c);
                    // Remain in TagOpen to collect the closing tag name next.
                }
                '!' | '?' => {
                    out.push(c);
                    state = S::Special;
                }
                _ if c.is_alphanumeric() || c == '_' => {
                    tag_buf.push(c);
                    state = S::TagName;
                }
                _ => {
                    out.push(c);
                    state = S::Text;
                }
            },
            S::TagName => match c {
                ':' => {
                    // Namespace prefix — discard everything collected so far.
                    tag_buf.clear();
                }
                '>' => {
                    out.push_str(&tag_buf);
                    tag_buf.clear();
                    out.push(c);
                    state = S::Text;
                }
                ' ' | '\t' | '\n' | '\r' | '/' => {
                    out.push_str(&tag_buf);
                    tag_buf.clear();
                    out.push(c);
                    state = S::Attr;
                }
                _ => {
                    tag_buf.push(c);
                }
            },
            S::Attr => match c {
                '"' => {
                    out.push(c);
                    state = S::AttrVal(b'"');
                }
                '\'' => {
                    out.push(c);
                    state = S::AttrVal(b'\'');
                }
                '>' => {
                    out.push(c);
                    state = S::Text;
                }
                _ => {
                    out.push(c);
                }
            },
            S::AttrVal(delim) => {
                out.push(c);
                if c as u8 == delim {
                    state = S::Attr;
                }
            }
            S::Special => {
                out.push(c);
                if c == '>' {
                    state = S::Text;
                }
            }
        }
    }

    if !tag_buf.is_empty() {
        out.push_str(&tag_buf);
    }

    out
}

/// Extract the XML namespace URI from the root `Document` element.
///
/// Returns the first `xmlns="…"` or `xmlns:…="…"` value found.
/// Returns `None` when no namespace declaration is found.
pub(crate) fn xml_detect_ns(xml: &str) -> Option<String> {
    // Look for xmlns="..." or xmlns:prefix="..."
    let mut pos = 0;
    while let Some(idx) = xml[pos..].find("xmlns") {
        let abs = pos + idx;
        let rest = &xml[abs + 5..]; // skip "xmlns"
        // Accept xmlns=" or xmlns:something="
        let after_eq = if let Some(stripped) = rest.strip_prefix("=\"") {
            stripped
        } else if let Some(first_quote) = rest.find('"') {
            // xmlns:prefix="uri" — skip past the `=` before the opening `"`
            if rest[..first_quote].contains('=') {
                &rest[first_quote + 1..]
            } else {
                pos = abs + 1;
                continue;
            }
        } else {
            pos = abs + 1;
            continue;
        };
        let end = after_eq.find('"')?;
        return Some(after_eq[..end].to_owned());
    }
    None
}

// ── element extraction ────────────────────────────────────────────────────────

// ── zero-alloc XML write helpers ──────────────────────────────────────────────

/// Write `s` into `w` with XML character escaping — no heap allocation.
///
/// Scans for `&`, `<`, `>`, `"`, `'` and emits the corresponding entity between
/// runs of unescaped bytes, so typical SEPA names ("Max Mustermann GmbH") produce
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

// ── element extraction ────────────────────────────────────────────────────────

/// Return the trimmed text content of the **first** `<tag>…</tag>` element.
///
/// Handles tags with attributes: `<InstdAmt Ccy="EUR">100.00</InstdAmt>` → `"100.00"`.
/// Returns `None` when the tag is absent.
pub(crate) fn xml_text<'a>(src: &'a str, tag: &str) -> Option<&'a str> {
    let close = format!("</{tag}>");
    let open_pos = find_open(src, tag, 0)?;
    let gt = src[open_pos..].find('>')?;
    let content_start = open_pos + gt + 1;
    let content_end = src[content_start..].find(&close)?;
    Some(src[content_start..content_start + content_end].trim())
}

/// Return the trimmed **inner XML** of the first `<tag>…</tag>` block,
/// correctly tracking nesting depth.
///
/// Returns `None` when the tag is absent.
pub(crate) fn xml_inner<'a>(src: &'a str, tag: &str) -> Option<&'a str> {
    let close = format!("</{tag}>");
    let open_pos = find_open(src, tag, 0)?;
    let gt = src[open_pos..].find('>')?;
    let content_start = open_pos + gt + 1;
    let end = find_close_depth(src, content_start, tag, &close)?;
    Some(src[content_start..end].trim())
}

/// An iterator over inner-XML slices for every top-level `<tag>…</tag>` match.
///
/// Yields `&'src str` slices borrowed directly from the source string —
/// **zero heap allocations** per element, compared to [`xml_each`] which
/// allocates one `String` per element.
///
/// The close-tag sentinel (`</tag>`) is allocated once at construction.
pub(crate) struct XmlEachIter<'src> {
    src: &'src str,
    /// `"</TAG>"` — owned so the struct has only one lifetime parameter.
    close: String,
    pos: usize,
}

impl<'src> Iterator for XmlEachIter<'src> {
    type Item = &'src str;

    fn next(&mut self) -> Option<&'src str> {
        // Extract bare tag name from the close sentinel: "</TAG>" → "TAG"
        let tag = &self.close[2..self.close.len() - 1];
        let close_len = self.close.len();

        let open_pos = find_open(self.src, tag, self.pos)?;
        let gt = self.src[open_pos..].find('>')?;
        let content_start = open_pos + gt + 1;
        let end = find_close_depth(self.src, content_start, tag, &self.close)?;
        let content = self.src[content_start..end].trim();
        self.pos = end + close_len;
        Some(content)
    }
}

/// Return an iterator over inner-XML slices for every `<tag>…</tag>` block.
///
/// Prefer this over [`xml_each`] in hot paths: items are `&str` borrowed from
/// `src` with no per-element allocation.
pub(crate) fn xml_each_iter<'src>(src: &'src str, tag: &str) -> XmlEachIter<'src> {
    XmlEachIter {
        src,
        close: format!("</{tag}>"),
        pos: 0,
    }
}

/// Collect every `<tag>…</tag>` inner-XML block into an owned `Vec<String>`.
///
/// For streaming iteration without allocation, use [`xml_each_iter`] instead.
/// This function is provided for tests and one-shot convenience use.
#[cfg(test)]
pub(crate) fn xml_each(src: &str, tag: &str) -> Vec<String> {
    xml_each_iter(src, tag).map(str::to_owned).collect()
}

// ── private helpers ───────────────────────────────────────────────────────────

/// Find the byte position of the first `<tag>` or `<tag ` opening in `src`
/// starting from `from`.  Returns the absolute byte offset of the `<`.
fn find_open(src: &str, tag: &str, from: usize) -> Option<usize> {
    let mut pos = from;
    while pos < src.len() {
        let rel_lt = src[pos..].find('<')?;
        let lt = pos + rel_lt;
        let rest = &src[lt + 1..];
        if rest.starts_with(tag) {
            // Accept anything that cannot appear in an element name: `>`, whitespace
            let after_byte = rest.as_bytes().get(tag.len()).copied();
            if matches!(
                after_byte,
                Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'/')
            ) {
                return Some(lt);
            }
        }
        pos = lt + 1;
    }
    None
}

/// Find the byte position of the matching `close` tag starting at `from`,
/// while respecting nesting (depth tracking for same-name nested elements).
fn find_close_depth(src: &str, from: usize, tag: &str, close: &str) -> Option<usize> {
    let close_len = close.len();
    let mut depth = 1usize;
    let mut pos = from;
    loop {
        let next_open = find_open(src, tag, pos);
        let next_close = src[pos..].find(close).map(|p| p + pos);
        match (next_open, next_close) {
            (_, None) => return None,
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                // Skip past the entire opening tag to avoid re-matching it.
                let gt = src[o..].find('>').unwrap_or(0);
                pos = o + gt + 1;
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(c);
                }
                pos = c + close_len;
            }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

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

    #[test]
    fn normalize_strips_prefix() {
        let input = r#"<ns2:TxSts>RJCT</ns2:TxSts>"#;
        assert_eq!(normalize_ns(input), "<TxSts>RJCT</TxSts>");
    }

    #[test]
    fn normalize_preserves_attrs() {
        let input = r#"<InstdAmt Ccy="EUR">100.00</InstdAmt>"#;
        assert_eq!(normalize_ns(input), input);
    }

    #[test]
    fn normalize_xmlns_attr_preserved() {
        let input = r#"<Document xmlns:ns2="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03"><ns2:Foo>bar</ns2:Foo></Document>"#;
        let out = normalize_ns(input);
        // Element prefix stripped, attribute value untouched
        assert!(out.contains("<Foo>bar</Foo>"));
        assert!(out.contains(r#"xmlns:ns2="urn:"#));
    }

    #[test]
    fn normalize_xml_declaration_preserved() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?><Root/>"#;
        assert_eq!(normalize_ns(input), input);
    }

    #[test]
    fn normalize_noop_when_no_prefix() {
        let src = "<TxSts>RJCT</TxSts>";
        assert_eq!(normalize_ns(src), src);
    }

    // ── xml_detect_ns ─────────────────────────────────────────────────────────

    #[test]
    fn detect_ns_default_namespace() {
        let xml = r#"<Document xmlns="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">"#;
        assert_eq!(
            xml_detect_ns(xml).as_deref(),
            Some("urn:iso:std:iso:20022:tech:xsd:pain.002.003.03")
        );
    }

    #[test]
    fn detect_ns_prefixed_namespace() {
        let xml = r#"<ns2:Document xmlns:ns2="urn:iso:std:iso:20022:tech:xsd:pain.002.003.03">"#;
        assert!(
            xml_detect_ns(xml)
                .unwrap_or_default()
                .contains("pain.002.003.03")
        );
    }

    #[test]
    fn detect_ns_missing() {
        assert_eq!(xml_detect_ns("<Root/>"), None);
    }

    // ── xml_text ──────────────────────────────────────────────────────────────

    #[test]
    fn text_simple() {
        assert_eq!(xml_text("<TxSts>RJCT</TxSts>", "TxSts"), Some("RJCT"));
    }

    #[test]
    fn text_with_attribute() {
        assert_eq!(
            xml_text(r#"<InstdAmt Ccy="EUR">100.00</InstdAmt>"#, "InstdAmt"),
            Some("100.00")
        );
    }

    #[test]
    fn text_first_of_two() {
        let src = "<Cd>OPBD</Cd><Cd>CLBD</Cd>";
        assert_eq!(xml_text(src, "Cd"), Some("OPBD"));
    }

    #[test]
    fn text_absent() {
        assert_eq!(xml_text("<Foo>bar</Foo>", "Baz"), None);
    }

    #[test]
    fn text_whitespace_trimmed() {
        assert_eq!(
            xml_text("<MsgId>  HELLO-123  </MsgId>", "MsgId"),
            Some("HELLO-123")
        );
    }

    // ── xml_inner ─────────────────────────────────────────────────────────────

    #[test]
    fn inner_flat() {
        let src = "<Bal><Cd>OPBD</Cd><Amt>100.00</Amt></Bal>";
        assert_eq!(
            xml_inner(src, "Bal"),
            Some("<Cd>OPBD</Cd><Amt>100.00</Amt>")
        );
    }

    #[test]
    fn inner_nested_same_tag() {
        let src = "<A><A>inner</A></A>";
        assert_eq!(xml_inner(src, "A"), Some("<A>inner</A>"));
    }

    // ── xml_each ──────────────────────────────────────────────────────────────

    #[test]
    fn each_two_blocks() {
        let src = "<Ntry><Amt>1.00</Amt></Ntry><Ntry><Amt>2.00</Amt></Ntry>";
        let blocks = xml_each(src, "Ntry");
        assert_eq!(blocks.len(), 2);
        assert_eq!(xml_text(&blocks[0], "Amt"), Some("1.00"));
        assert_eq!(xml_text(&blocks[1], "Amt"), Some("2.00"));
    }

    #[test]
    fn each_empty() {
        assert_eq!(xml_each("<Foo/>", "Ntry"), Vec::<String>::new());
    }

    #[test]
    fn each_three_tx_blocks() {
        let src = "<R><TxSts>RJCT</TxSts></R><R><TxSts>ACTC</TxSts></R><R><TxSts>RJCT</TxSts></R>";
        let blocks = xml_each(src, "R");
        assert_eq!(blocks.len(), 3);
        assert_eq!(xml_text(&blocks[2], "TxSts"), Some("RJCT"));
    }

    // ── xml_each_iter ─────────────────────────────────────────────────────────

    #[test]
    fn each_iter_yields_same_as_xml_each() {
        let src = "<Ntry><Amt>1.00</Amt></Ntry><Ntry><Amt>2.00</Amt></Ntry>";
        let from_vec: Vec<&str> = xml_each_iter(src, "Ntry").collect();
        let from_each = xml_each(src, "Ntry");
        assert_eq!(
            from_vec,
            from_each.iter().map(String::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn each_iter_zero_alloc_check() {
        // Verify iterator yields borrowed slices (no .to_owned() needed for parsing)
        let src = "<Tx><Id>TX-001</Id></Tx><Tx><Id>TX-002</Id></Tx>";
        let ids: Vec<&str> = xml_each_iter(src, "Tx")
            .filter_map(|block| xml_text(block, "Id"))
            .collect();
        assert_eq!(ids, ["TX-001", "TX-002"]);
    }

    #[test]
    fn each_iter_empty() {
        assert_eq!(xml_each_iter("<Foo/>", "Bar").count(), 0);
    }
}
