//! Namespace-agnostic XML tree used by the ISO 20022 parsers.
//!
//! ISO 20022 documents arrive from banks in two shapes — with a default
//! namespace (`<Document xmlns="urn:…">`) or with a prefix
//! (`<ns2:Document xmlns:ns2="urn:…">`) — and the same bank may switch between
//! them across message types. Rather than match on qualified names, this module
//! parses into a tree keyed by **local names**, so `Document` and `ns2:Document`
//! are indistinguishable to callers.
//!
//! Parsing is delegated to [`quick_xml`], which handles the parts a
//! string-scanning approach silently gets wrong: comments (a commented-out
//! `<MsgId>` must not be read as a real one), entity references (`&amp;` must
//! decode to `&`), CDATA sections, self-closing tags, and attribute values that
//! contain angle brackets.
//!
//! ## Security posture
//!
//! `quick_xml` never resolves external entities, so this parser is not
//! vulnerable to XXE. Defence in depth on top of that:
//!
//! - `<!DOCTYPE …>` is rejected outright ([`XmlError::DoctypeNotAllowed`]).
//!   No conformant ISO 20022 document contains one, and rejecting it forecloses
//!   entity-expansion ("billion laughs") attacks.
//! - Nesting is capped at [`MAX_DEPTH`] ([`XmlError::TooDeep`]). The tree is
//!   built with an explicit stack, so deep input cannot overflow the call stack,
//!   but the cap bounds memory on hostile input.
//! - Unknown entity references are an error rather than being passed through
//!   undecoded — for payment data, silently emitting `&foo;` into a name field
//!   is worse than failing.

use quick_xml::events::Event;

/// Maximum element nesting depth accepted by [`Document::parse`].
///
/// Real ISO 20022 messages nest around 15 levels deep; 256 leaves ample
/// headroom while bounding memory on adversarial input.
pub(crate) const MAX_DEPTH: usize = 256;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when an XML document cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XmlError {
    /// The document is not well-formed XML.
    #[error("malformed XML: {0}")]
    Malformed(String),

    /// The document contains a `<!DOCTYPE …>` declaration, which is rejected.
    ///
    /// ISO 20022 messages never carry a DTD; accepting one would open the door
    /// to entity-expansion attacks.
    #[error("XML DOCTYPE declarations are not accepted in ISO 20022 messages")]
    DoctypeNotAllowed,

    /// Element nesting exceeded the parser's depth limit of 256 levels.
    ///
    /// Real ISO 20022 messages nest around 15 levels deep; the cap bounds
    /// memory use on adversarial input.
    #[error("XML nesting deeper than {MAX_DEPTH} levels")]
    TooDeep,

    /// The document contains no root element.
    #[error("XML document has no root element")]
    NoRootElement,

    /// The document contains more than one root element.
    ///
    /// Not well-formed XML. Rejecting it prevents a trailing second
    /// `<Document>` from silently displacing the real one.
    #[error("XML document has more than one root element")]
    MultipleRootElements,

    /// An entity reference that is not one of the five XML predefined entities.
    ///
    /// Resolving it would require a DTD, and DTDs are rejected — see
    /// [`XmlError::DoctypeNotAllowed`].
    #[error("unknown XML entity reference: &{0};")]
    UnknownEntity(String),
}

// ── Node ──────────────────────────────────────────────────────────────────────

/// One element in a parsed XML document.
///
/// Element and attribute names are **local names**: any namespace prefix has
/// been stripped, so `<ns2:MsgId>` and `<MsgId>` both yield `name == "MsgId"`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Node {
    /// Local element name, namespace prefix removed.
    pub(crate) name: String,
    /// Concatenated, entity-decoded, trimmed text content directly inside this
    /// element (text inside child elements is not included).
    pub(crate) text: String,
    /// Attributes as `(local name, decoded value)` pairs.
    pub(crate) attrs: Vec<(String, String)>,
    /// Direct child elements, in document order.
    pub(crate) children: Vec<Node>,
}

impl Node {
    /// The first **direct child** named `name`, or `None`.
    ///
    /// Prefer this over [`descendant`](Self::descendant) whenever the schema
    /// fixes the element's position: it cannot accidentally match a
    /// like-named element nested deeper in the tree.
    pub(crate) fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// All direct children named `name`, in document order.
    pub(crate) fn children_named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The first descendant named `name`, searched depth-first, or `None`.
    ///
    /// Use only where the schema genuinely allows the element at varying
    /// depths; otherwise [`child`](Self::child) or [`path`](Self::path) is safer.
    pub(crate) fn descendant(&self, name: &str) -> Option<&Node> {
        // Explicit stack: bounded by MAX_DEPTH, but recursion-free by
        // construction. Nodes are tested as they are popped — testing a whole
        // sibling row before descending would return the *shallowest* match
        // rather than the first one in document order.
        let mut stack: Vec<&Node> = self.children.iter().rev().collect();
        while let Some(n) = stack.pop() {
            if n.name == name {
                return Some(n);
            }
            stack.extend(n.children.iter().rev());
        }
        None
    }

    /// Follow a chain of direct-child names, e.g. `path(&["Acct", "Id", "IBAN"])`.
    pub(crate) fn path(&self, names: &[&str]) -> Option<&Node> {
        names.iter().try_fold(self, |node, n| node.child(n))
    }

    /// Trimmed text of the element at `path`, or `None` if absent or empty.
    pub(crate) fn text_at(&self, path: &[&str]) -> Option<&str> {
        self.path(path)
            .map(|n| n.text.as_str())
            .filter(|t| !t.is_empty())
    }

    /// Trimmed text of the first direct child named `name`, or `None` if absent
    /// or empty.
    pub(crate) fn text_of(&self, name: &str) -> Option<&str> {
        self.text_at(&[name])
    }

    /// Trimmed text of the first descendant named `name`, or `None`.
    pub(crate) fn text_of_descendant(&self, name: &str) -> Option<&str> {
        self.descendant(name)
            .map(|n| n.text.as_str())
            .filter(|t| !t.is_empty())
    }

    /// Value of attribute `name` on this element.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Text of a code element that may be either a bare code or a
    /// `Cd`/`Prtry` choice.
    ///
    /// ISO 20022 changed several fields from a plain code to a choice between
    /// releases — `camt.053` `Ntry/Sts` is `<Sts>BOOK</Sts>` up to `.001.02`
    /// but `<Sts><Cd>BOOK</Cd></Sts>` from `.001.08`. This accepts both.
    pub(crate) fn code(&self) -> Option<&str> {
        // A `Cd`/`Prtry` child wins over stray text: on malformed input such as
        // `<Sts>STRAY<Cd>BOOK</Cd></Sts>` the structured value is the one to
        // trust. Bare text is the older, choice-free encoding.
        self.text_of("Cd")
            .or_else(|| self.text_of("Prtry"))
            .or_else(|| {
                Some(&self.text)
                    .filter(|t| !t.is_empty())
                    .map(String::as_str)
            })
    }
}

// ── Document ──────────────────────────────────────────────────────────────────

/// A parsed XML document: its root element plus the detected namespace URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Document {
    /// The root element (e.g. `Document`).
    pub(crate) root: Node,
    /// The namespace URI declared on the root element, if any.
    pub(crate) namespace: Option<String>,
}

impl Document {
    /// Parse `xml` into a namespace-agnostic tree.
    ///
    /// # Errors
    ///
    /// Returns [`XmlError`] when the input is not well-formed, contains a
    /// DOCTYPE, or nests deeper than [`MAX_DEPTH`].
    pub(crate) fn parse(xml: &str) -> Result<Self, XmlError> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let config = reader.config_mut();
        // Text must NOT be trimmed per event. quick-xml delivers `&amp;` as a
        // separate `GeneralRef` event, so "Max &amp; Co" arrives as
        // Text("Max ") + Ref("amp") + Text(" Co"); trimming each part would
        // fuse it into "Max&Co". Text is instead trimmed once, on End.
        config.trim_text(false);
        config.expand_empty_elements = false;
        config.check_end_names = true;

        // `stack` holds the open elements; `stack[0]` becomes the root.
        let mut stack: Vec<Node> = Vec::new();
        let mut root: Option<Node> = None;

        loop {
            match reader
                .read_event()
                .map_err(|e| XmlError::Malformed(e.to_string()))?
            {
                Event::Start(e) => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(XmlError::TooDeep);
                    }
                    stack.push(element(&e)?);
                }
                Event::Empty(e) => {
                    let node = element(&e)?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        // A self-closing root element: `<Document/>`.
                        None => root = Some(node),
                    }
                }
                Event::End(_) => {
                    let Some(mut node) = stack.pop() else {
                        continue;
                    };
                    // Trim once, now that every text run and entity reference
                    // for this element has been appended.
                    let trimmed = node.text.trim();
                    if trimmed.len() != node.text.len() {
                        // Truncate/shift in place rather than reallocating.
                        let start = trimmed.as_ptr() as usize - node.text.as_ptr() as usize;
                        let end = start + trimmed.len();
                        node.text.replace_range(..start, "");
                        node.text.truncate(end - start);
                    }
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => set_root(&mut root, node)?,
                    }
                }
                Event::Text(e) => {
                    if let Some(node) = stack.last_mut() {
                        // Entity references arrive as `GeneralRef`, so text is
                        // already literal — unescaping here would double-decode.
                        let decoded = e
                            .decode()
                            .map_err(|err| XmlError::Malformed(err.to_string()))?;
                        node.text.push_str(&decoded);
                    }
                }
                Event::GeneralRef(e) => {
                    if let Some(node) = stack.last_mut() {
                        let raw = e
                            .decode()
                            .map_err(|err| XmlError::Malformed(err.to_string()))?;
                        if let Some(ch) = e
                            .resolve_char_ref()
                            .map_err(|err| XmlError::Malformed(err.to_string()))?
                        {
                            // Numeric character reference: &#252; / &#x26;
                            node.text.push(ch);
                        } else if let Some(text) =
                            quick_xml::escape::resolve_predefined_entity(&raw)
                        {
                            // One of the five XML predefined entities.
                            node.text.push_str(text);
                        } else {
                            // Only a DTD could define this, and DTDs are
                            // rejected — so it can never be resolved. Failing is
                            // safer than writing "&foo;" into a payment field.
                            return Err(XmlError::UnknownEntity(raw.into_owned()));
                        }
                    }
                }
                Event::CData(e) => {
                    if let Some(node) = stack.last_mut() {
                        let decoded = e
                            .decode()
                            .map_err(|err| XmlError::Malformed(err.to_string()))?;
                        node.text.push_str(&decoded);
                    }
                }
                Event::DocType(_) => return Err(XmlError::DoctypeNotAllowed),
                // Comments, processing instructions and the XML declaration
                // carry no message data and are skipped.
                Event::Comment(_) | Event::PI(_) | Event::Decl(_) => {}
                Event::Eof => break,
            }
        }

        let root = root.ok_or(XmlError::NoRootElement)?;
        let namespace = root
            .attrs
            .iter()
            .find(|(k, _)| k == "xmlns")
            .map(|(_, v)| v.clone());

        Ok(Self { root, namespace })
    }
}

/// Record the document's single root element.
///
/// A well-formed XML document has exactly one. Accepting a second and letting
/// it overwrite the first is a document-substitution hazard: an attacker who can
/// append `<Document>…</Document>` to a file makes this library read different
/// data than the validator or the next consumer in the chain saw.
fn set_root(root: &mut Option<Node>, node: Node) -> Result<(), XmlError> {
    if root.is_some() {
        return Err(XmlError::MultipleRootElements);
    }
    *root = Some(node);
    Ok(())
}

/// Build a [`Node`] from a start/empty tag, stripping namespace prefixes.
fn element(e: &quick_xml::events::BytesStart<'_>) -> Result<Node, XmlError> {
    let name = local_name(e.name().as_ref())?.to_owned();

    let mut attrs = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| XmlError::Malformed(err.to_string()))?;
        // `xmlns:ns2="…"` is recorded under its local name `ns2`; the default
        // declaration `xmlns="…"` keeps the key `xmlns`.
        let key = local_name(attr.key.as_ref())?.to_owned();
        // Attribute-value normalisation per the XML 1.0 spec, resolving the
        // five predefined entities (so `Note="a &lt; b"` yields `a < b`).
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| XmlError::Malformed(err.to_string()))?
            .into_owned();
        attrs.push((key, value));
    }

    Ok(Node {
        name,
        attrs,
        ..Node::default()
    })
}

/// Strip an optional `prefix:` from a qualified name.
fn local_name(raw: &[u8]) -> Result<&str, XmlError> {
    let s = std::str::from_utf8(raw)
        .map_err(|_| XmlError::Malformed("element name is not valid UTF-8".to_owned()))?;
    Ok(match s.split_once(':') {
        Some((_, local)) => local,
        None => s,
    })
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> Node {
        Document::parse(xml).unwrap().root
    }

    #[test]
    fn strips_namespace_prefixes() {
        let a = parse(r#"<Doc xmlns="urn:x"><MsgId>M1</MsgId></Doc>"#);
        let b = parse(r#"<n:Doc xmlns:n="urn:x"><n:MsgId>M1</n:MsgId></n:Doc>"#);
        assert_eq!(a.text_of("MsgId"), Some("M1"));
        assert_eq!(b.text_of("MsgId"), Some("M1"));
        assert_eq!(a.name, "Doc");
        assert_eq!(b.name, "Doc");
    }

    #[test]
    fn comments_are_not_elements() {
        // Regression: the previous string scanner returned "COMMENTED".
        let n = parse("<R><!-- <MsgId>COMMENTED</MsgId> --><MsgId>REAL</MsgId></R>");
        assert_eq!(n.text_of("MsgId"), Some("REAL"));
        assert_eq!(n.children_named("MsgId").count(), 1);
    }

    #[test]
    fn entities_are_decoded() {
        // Regression: the previous scanner returned "Ben &amp; Jerry&apos;s".
        let n = parse("<R><Nm>Ben &amp; Jerry&apos;s &lt;GmbH&gt;</Nm></R>");
        assert_eq!(n.text_of("Nm"), Some("Ben & Jerry's <GmbH>"));
    }

    #[test]
    fn numeric_character_references_are_decoded() {
        let n = parse("<R><Nm>M&#252;ller &#x26; Co</Nm></R>");
        assert_eq!(n.text_of("Nm"), Some("Müller & Co"));
    }

    #[test]
    fn unknown_entity_is_an_error() {
        // Better to fail loudly than to write "&oops;" into a payment field.
        assert_eq!(
            Document::parse("<R><Nm>&oops;</Nm></R>"),
            Err(XmlError::UnknownEntity("oops".to_owned()))
        );
    }

    #[test]
    fn cdata_is_read_as_text() {
        let n = parse("<R><Nm><![CDATA[A & B <raw>]]></Nm></R>");
        assert_eq!(n.text_of("Nm"), Some("A & B <raw>"));
    }

    #[test]
    fn self_closing_element_is_empty_not_a_swallowed_sibling() {
        // Regression: the previous scanner scanned past `<Nm/>` to a later close tag.
        let n = parse("<R><Dbtr><Nm/></Dbtr><Cdtr><Nm>Real Name</Nm></Cdtr></R>");
        assert_eq!(n.path(&["Dbtr", "Nm"]).map(|x| x.text.as_str()), Some(""));
        assert_eq!(n.text_of("Dbtr"), None);
        assert_eq!(n.path(&["Cdtr", "Nm"]).unwrap().text, "Real Name");
    }

    #[test]
    fn attribute_values_containing_markup_do_not_confuse_the_parser() {
        let n = parse(r#"<R><Amt Ccy="EUR" Note="a &lt; b">100.00</Amt></R>"#);
        let amt = n.child("Amt").unwrap();
        assert_eq!(amt.text, "100.00");
        assert_eq!(amt.attr("Ccy"), Some("EUR"));
        assert_eq!(amt.attr("Note"), Some("a < b"));
    }

    #[test]
    fn child_is_direct_only_but_descendant_searches_deep() {
        let n = parse("<R><Acct><Id><IBAN>DE00</IBAN></Id></Acct></R>");
        assert!(n.child("IBAN").is_none());
        assert_eq!(n.descendant("IBAN").map(|x| x.text.as_str()), Some("DE00"));
        assert_eq!(n.text_at(&["Acct", "Id", "IBAN"]), Some("DE00"));
    }

    #[test]
    fn child_does_not_match_a_deeper_like_named_element() {
        // `Stmt/Id` must not be satisfied by `Stmt/Acct/Id`.
        let n = parse("<Stmt><Acct><Id><IBAN>DE00</IBAN></Id></Acct></Stmt>");
        assert_eq!(n.text_of("Id"), None);
    }

    #[test]
    fn nested_same_name_elements_keep_their_structure() {
        let n = parse("<Dt><Dt>2026-07-13</Dt></Dt>");
        assert_eq!(n.text_of("Dt"), Some("2026-07-13"));
    }

    #[test]
    fn repeated_children_are_all_kept_in_order() {
        let n = parse("<R><Ntry><Amt>1.00</Amt></Ntry><Ntry><Amt>2.00</Amt></Ntry></R>");
        let amounts: Vec<&str> = n
            .children_named("Ntry")
            .filter_map(|e| e.text_of("Amt"))
            .collect();
        assert_eq!(amounts, ["1.00", "2.00"]);
    }

    #[test]
    fn structured_code_wins_over_stray_text() {
        let mixed = parse("<Sts>STRAY<Cd>BOOK</Cd></Sts>");
        assert_eq!(mixed.code(), Some("BOOK"));
    }

    #[test]
    fn descendant_returns_the_first_match_in_document_order() {
        let n = parse("<R><A><X>first</X></A><X>second</X></R>");
        assert_eq!(n.descendant("X").map(|x| x.text.as_str()), Some("first"));
    }

    #[test]
    fn multiple_root_elements_are_rejected() {
        // Document-substitution hazard: a trailing second root must not win.
        let xml = "<Doc xmlns='urn:x'><MsgId>REAL</MsgId></Doc>\
                   <Doc xmlns='urn:evil'><MsgId>EVIL</MsgId></Doc>";
        assert_eq!(Document::parse(xml), Err(XmlError::MultipleRootElements));
    }

    #[test]
    fn unmatched_end_tag_is_rejected() {
        assert!(matches!(
            Document::parse("</oops><R>x</R>"),
            Err(XmlError::Malformed(_))
        ));
    }

    #[test]
    fn code_accepts_bare_and_choice_forms() {
        // camt.053.001.02 style
        let old = parse("<Sts>BOOK</Sts>");
        assert_eq!(old.code(), Some("BOOK"));
        // camt.053.001.08 style
        let new = parse("<Sts><Cd>BOOK</Cd></Sts>");
        assert_eq!(new.code(), Some("BOOK"));
        // proprietary fallback
        let prtry = parse("<Rsn><Prtry>BANK-007</Prtry></Rsn>");
        assert_eq!(prtry.code(), Some("BANK-007"));
    }

    #[test]
    fn namespace_is_detected_for_both_shapes() {
        assert_eq!(
            Document::parse(r#"<Document xmlns="urn:iso:x"><A/></Document>"#)
                .unwrap()
                .namespace
                .as_deref(),
            Some("urn:iso:x")
        );
        // A prefixed declaration is recorded under the prefix, not `xmlns`.
        let doc =
            Document::parse(r#"<n:Document xmlns:n="urn:iso:y"><n:A/></n:Document>"#).unwrap();
        assert_eq!(doc.root.attr("n"), Some("urn:iso:y"));
    }

    #[test]
    fn doctype_is_rejected() {
        // Defence in depth against entity-expansion attacks.
        let xml = "<!DOCTYPE r [<!ENTITY a 'boom'>]><r>&a;</r>";
        assert_eq!(Document::parse(xml), Err(XmlError::DoctypeNotAllowed));
    }

    #[test]
    fn excessive_nesting_is_rejected() {
        let deep = "<a>".repeat(MAX_DEPTH + 10);
        assert_eq!(Document::parse(&deep), Err(XmlError::TooDeep));
    }

    #[test]
    fn malformed_xml_is_rejected() {
        assert!(matches!(
            Document::parse("<a><b></a></b>"),
            Err(XmlError::Malformed(_))
        ));
        assert!(matches!(Document::parse(""), Err(XmlError::NoRootElement)));
    }

    #[test]
    fn entity_in_the_middle_of_a_word_is_not_split() {
        // quick-xml emits Text("AT") + Ref("amp") + Text("T"); naive
        // per-event trimming or space-joining would corrupt this.
        assert_eq!(parse("<Nm>AT&amp;T</Nm>").text, "AT&T");
        assert_eq!(parse("<Nm>Max &amp; Co</Nm>").text, "Max & Co");
        assert_eq!(parse("<Nm>  Max &amp; Co  </Nm>").text, "Max & Co");
    }

    #[test]
    fn whitespace_is_trimmed() {
        let n = parse("<R><MsgId>  HELLO-123  </MsgId></R>");
        assert_eq!(n.text_of("MsgId"), Some("HELLO-123"));
    }

    #[test]
    fn empty_text_reads_as_none() {
        let n = parse("<R><MsgId></MsgId><Other/></R>");
        assert_eq!(n.text_of("MsgId"), None);
        assert_eq!(n.text_of("Other"), None);
    }
}
