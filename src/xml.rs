//! Namespace-resolved XML tree used by the ISO 20022 parsers.
//!
//! ISO 20022 documents arrive from banks in two shapes — with a default
//! namespace (`<Document xmlns="urn:…">`) or with a prefix
//! (`<ns2:Document xmlns:ns2="urn:…">`) — and the same bank may switch between
//! them across message types. Rather than match on qualified names, this module
//! parses into a tree keyed by local names and resolved namespace URIs. Equivalent
//! default/prefixed identities match; foreign lookalikes do not.
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
//!   and finite byte/node/text/attribute budgets bound retained message data.
//! - Unknown entity references are an error rather than being passed through
//!   undecoded — for payment data, silently emitting `&foo;` into a name field
//!   is worse than failing.

use quick_xml::events::Event;
use quick_xml::name::{NamespaceResolver, ResolveResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Finite resource limits applied before retaining XML message data.
///
/// Sizes are UTF-8 bytes, including decoded text/attribute values. The root is
/// one node at depth one. Namespace declarations count as attributes. Limits
/// do not promise an absolute CPU or process-RSS bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum input bytes.
    pub input_bytes: usize,
    /// Maximum retained elements, including empty elements.
    pub nodes: usize,
    /// Maximum depth, from one through the hard ceiling of 256.
    pub depth: usize,
    /// Maximum decoded text bytes in one element.
    pub text_bytes: usize,
    /// Maximum decoded text bytes across the document.
    pub total_text_bytes: usize,
    /// Maximum attributes on one element, including namespace declarations.
    pub attributes: usize,
    /// Maximum decoded bytes in one attribute value.
    pub attribute_bytes: usize,
    /// Maximum decoded attribute-value bytes across the document.
    pub total_attribute_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            input_bytes: 32 * 1024 * 1024,
            nodes: 250_000,
            depth: MAX_DEPTH,
            text_bytes: 1024 * 1024,
            total_text_bytes: 16 * 1024 * 1024,
            attributes: 64,
            attribute_bytes: 8 * 1024,
            total_attribute_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ParseLimits {
    fn validate(self) -> Result<(), XmlError> {
        if [
            self.input_bytes,
            self.nodes,
            self.depth,
            self.text_bytes,
            self.total_text_bytes,
            self.attributes,
            self.attribute_bytes,
            self.total_attribute_bytes,
        ]
        .contains(&0)
            || self.depth > MAX_DEPTH
        {
            return Err(XmlError::InvalidLimits);
        }
        Ok(())
    }
}

struct Budget {
    limits: ParseLimits,
    nodes: usize,
    text: usize,
    attributes: usize,
}

fn check_limit(resource: &'static str, actual: usize, limit: usize) -> Result<(), XmlError> {
    if actual > limit {
        Err(XmlError::LimitExceeded {
            resource,
            limit,
            actual,
        })
    } else {
        Ok(())
    }
}

fn charge(
    resource: &'static str,
    count: &mut usize,
    bytes: usize,
    limit: usize,
) -> Result<(), XmlError> {
    let next = count
        .checked_add(bytes)
        .ok_or_else(|| XmlError::Malformed("resource counter overflow".to_owned()))?;
    check_limit(resource, next, limit)?;
    *count = next;
    Ok(())
}

impl Budget {
    fn element(&mut self, depth: usize) -> Result<(), XmlError> {
        if depth > MAX_DEPTH {
            return Err(XmlError::TooDeep);
        }
        check_limit("depth", depth, self.limits.depth)?;
        charge("nodes", &mut self.nodes, 1, self.limits.nodes)
    }

    fn text(&mut self, node: &mut Node, text: &str) -> Result<(), XmlError> {
        check_xml_text(text)?;
        let size = node
            .text
            .len()
            .checked_add(text.len())
            .ok_or_else(|| XmlError::Malformed("text size overflow".to_owned()))?;
        check_limit("text_bytes", size, self.limits.text_bytes)?;
        charge(
            "total_text_bytes",
            &mut self.text,
            text.len(),
            self.limits.total_text_bytes,
        )?;
        node.text.push_str(text);
        Ok(())
    }
}

/// Maximum element nesting depth accepted by [`Document::parse`].
///
/// Real ISO 20022 messages nest around 15 levels deep; 256 leaves ample
/// headroom while limiting nesting; separate budgets bound retained data.
pub(crate) const MAX_DEPTH: usize = 256;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Error returned when an XML document cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XmlError {
    /// A configured limit is zero or exceeds the hard nesting ceiling.
    #[error("parse limits must be positive and depth must not exceed 256")]
    InvalidLimits,
    /// A finite input or retained-resource budget was exhausted.
    #[error("XML {resource} limit {limit} exceeded by count {actual}")]
    LimitExceeded {
        /// Resource name; sizes are UTF-8 bytes.
        resource: &'static str,
        /// Configured maximum.
        limit: usize,
        /// Attempted count.
        actual: usize,
    },
    /// The document wrapper, namespace or message body has the wrong identity.
    #[error("XML message identity does not match {expected}")]
    Identity {
        /// Required ISO 20022 message family.
        expected: &'static str,
    },
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
    /// The hard ceiling limits nesting; separate budgets bound retained data,
    /// not absolute process memory or execution time.
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
/// Elements retain resolved namespaces and local names; attributes retain
/// qualified spelling. Foreign-namespace lookalikes cannot supply standard fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Node {
    /// Resolved element namespace, shared rather than copied per element.
    pub(crate) namespace: Option<Arc<str>>,
    /// Local element name, namespace prefix removed.
    pub(crate) name: String,
    /// Concatenated, entity-decoded text content directly inside this
    /// element (text inside child elements is not included).
    pub(crate) text: String,
    /// Attributes as `(qualified name, decoded value)` pairs.
    pub(crate) attrs: Vec<(String, String)>,
    /// Direct child elements, in document order.
    pub(crate) children: Vec<Node>,
}

impl Node {
    /// The first **direct child** named `name`, or `None`.
    ///
    /// Use this whenever the schema fixes the element's position: it cannot match a
    /// like-named element nested deeper in the tree.
    pub(crate) fn child(&self, name: &str) -> Option<&Node> {
        self.children
            .iter()
            .find(|c| c.name == name && c.namespace == self.namespace)
    }

    /// All direct children named `name`, in document order.
    pub(crate) fn children_named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Node> {
        self.children
            .iter()
            .filter(move |c| c.name == name && c.namespace == self.namespace)
    }

    /// The first descendant named `name`, searched depth-first, or `None`.
    ///
    /// Use only where the schema genuinely allows the element at varying
    /// depths; otherwise [`child`](Self::child) or [`path`](Self::path) is safer.
    #[cfg(test)]
    pub(crate) fn descendant(&self, name: &str) -> Option<&Node> {
        // Explicit stack: bounded by MAX_DEPTH, but recursion-free by
        // construction. Nodes are tested as they are popped — testing a whole
        // sibling row before descending would return the *shallowest* match
        // rather than the first one in document order.
        let mut stack: Vec<&Node> = self.children.iter().rev().collect();
        while let Some(n) = stack.pop() {
            if n.namespace != self.namespace {
                continue;
            }
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
            .map(|n| n.text.trim())
            .filter(|t| !t.is_empty())
    }

    /// Trimmed text of the first direct child named `name`, or `None` if absent
    /// or empty.
    pub(crate) fn text_of(&self, name: &str) -> Option<&str> {
        self.text_at(&[name])
    }

    pub(crate) fn raw_text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|node| node.text.as_str())
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
        //
        // `Prtry` has two shapes across the camt choices and both are real.
        // In `BalanceType10Choice`, `EntryStatus1Choice` and
        // `ReturnReason5Choice` it is a `Max35Text`, so the code is its own
        // text. In `ChargeType3Choice` it is a `GenericIdentification3`, whose
        // `Id` is **mandatory** — so a schema-valid proprietary charge type is
        // `<Prtry><Id>RETURN_FEE</Id></Prtry>` and has no direct text at all.
        // Reading only the text shape returned `None` for every one of
        // those, discarding a value the file did carry.
        self.text_of("Cd")
            .or_else(|| self.text_of("Prtry"))
            .or_else(|| self.child("Prtry").and_then(|p| p.text_of("Id")))
            .or_else(|| self.child("Prtry").and_then(|p| p.text_of("Cd")))
            .or_else(|| Some(self.text.trim()).filter(|t| !t.is_empty()))
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
    /// Parse `xml` into a namespace-aware, finitely bounded tree.
    ///
    /// # Errors
    ///
    /// Returns [`XmlError`] when the input is not well-formed, contains a
    /// DOCTYPE, or nests deeper than [`MAX_DEPTH`].
    #[cfg(test)]
    pub(crate) fn parse(xml: &str) -> Result<Self, XmlError> {
        Self::parse_with_limits(xml, ParseLimits::default())
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse_with_limits(xml: &str, limits: ParseLimits) -> Result<Self, XmlError> {
        limits.validate()?;
        check_limit("input_bytes", xml.len(), limits.input_bytes)?;
        let mut budget = Budget {
            limits,
            nodes: 0,
            text: 0,
            attributes: 0,
        };
        let mut namespaces = HashMap::new();
        let mut reader = quick_xml::NsReader::from_str(xml);
        let config = reader.config_mut();
        // Text must NOT be trimmed per event. quick-xml delivers `&amp;` as a
        // separate `GeneralRef` event, so "Max &amp; Co" arrives as
        // Text("Max ") + Ref("amp") + Text(" Co"); trimming each part would
        // fuse it into "Max&Co". Text is instead trimmed once, on End.
        config.trim_text(false);
        config.expand_empty_elements = false;
        config.check_end_names = true;
        config.check_comments = true;

        // `stack` holds the open elements; `stack[0]` becomes the root.
        let mut stack: Vec<Node> = Vec::new();
        let mut root: Option<Node> = None;
        let mut declaration_allowed = true;

        loop {
            let event = reader
                .read_event()
                .map_err(|e| XmlError::Malformed(e.to_string()))?;
            if !matches!(event, Event::Decl(_)) {
                declaration_allowed = false;
            }
            match event {
                Event::Start(e) => {
                    if stack.is_empty() && root.is_some() {
                        return Err(XmlError::MultipleRootElements);
                    }
                    budget.element(stack.len() + 1)?;
                    stack.push(element(
                        &e,
                        reader.resolver(),
                        &mut namespaces,
                        &mut budget,
                    )?);
                }
                Event::Empty(e) => {
                    budget.element(stack.len() + 1)?;
                    let node = element(&e, reader.resolver(), &mut namespaces, &mut budget)?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        // A self-closing root element: `<Document/>`. It goes
                        // through `set_root` like any other, or a document
                        // beginning `<Document/>` could be displaced by a
                        // second root appended after it.
                        None => set_root(&mut root, node)?,
                    }
                }
                Event::End(_) => {
                    let Some(node) = stack.pop() else {
                        return Err(XmlError::Malformed("unmatched end element".to_owned()));
                    };
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => set_root(&mut root, node)?,
                    }
                }
                Event::Text(e) => {
                    let decoded = e
                        .xml10_content()
                        .map_err(|err| XmlError::Malformed(err.to_string()))?;
                    check_xml_text(&decoded)?;
                    if let Some(node) = stack.last_mut() {
                        // Entity references arrive as `GeneralRef`, so text is
                        // already literal — unescaping here would double-decode.
                        budget.text(node, &decoded)?;
                    } else if !decoded
                        .chars()
                        .all(|character| matches!(character, ' ' | '\t' | '\r' | '\n'))
                    {
                        return Err(XmlError::Malformed(
                            "text outside the root element".to_owned(),
                        ));
                    }
                }
                Event::GeneralRef(e) => {
                    if stack.is_empty() {
                        return Err(XmlError::Malformed(
                            "entity outside the root element".to_owned(),
                        ));
                    }
                    if let Some(node) = stack.last_mut() {
                        budget.text(node, &reference_text(&e)?)?;
                    }
                }
                Event::CData(e) => {
                    if stack.is_empty() {
                        return Err(XmlError::Malformed(
                            "CDATA outside the root element".to_owned(),
                        ));
                    }
                    if let Some(node) = stack.last_mut() {
                        let decoded = e
                            .xml10_content()
                            .map_err(|err| XmlError::Malformed(err.to_string()))?;
                        check_xml_text(&decoded)?;
                        budget.text(node, &decoded)?;
                    }
                }
                Event::DocType(_) => return Err(XmlError::DoctypeNotAllowed),
                Event::Decl(declaration) => {
                    if !declaration_allowed {
                        return Err(XmlError::Malformed(
                            "invalid or misplaced XML declaration".to_owned(),
                        ));
                    }
                    check_declaration(&declaration, xml.is_ascii())?;
                    declaration_allowed = false;
                }
                Event::PI(instruction) => {
                    check_xml_text(
                        std::str::from_utf8(instruction.as_ref())
                            .map_err(|error| XmlError::Malformed(error.to_string()))?,
                    )?;
                    let target = std::str::from_utf8(instruction.target())
                        .map_err(|error| XmlError::Malformed(error.to_string()))?;
                    if target.eq_ignore_ascii_case("xml") || !is_xml_name(target, true) {
                        return Err(XmlError::Malformed(
                            "invalid or reserved processing-instruction target".to_owned(),
                        ));
                    }
                }
                Event::Comment(comment) => {
                    check_xml_text(
                        &comment
                            .xml10_content()
                            .map_err(|error| XmlError::Malformed(error.to_string()))?,
                    )?;
                }
                Event::Eof => {
                    if !stack.is_empty() {
                        return Err(XmlError::Malformed("unfinished XML element".to_owned()));
                    }
                    break;
                }
            }
        }

        let root = root.ok_or(XmlError::NoRootElement)?;
        let namespace = root.namespace.as_deref().map(str::to_owned);

        Ok(Self { root, namespace })
    }

    pub(crate) fn message(
        &self,
        family: &'static str,
        body: &str,
        versions: &[&str],
    ) -> Result<&Node, XmlError> {
        let identity_error = || XmlError::Identity { expected: family };
        if self.root.name != "Document"
            || self.root.children.len() != 1
            || !self.root.text.trim().is_empty()
        {
            return Err(identity_error());
        }
        let namespace = self.namespace.as_deref().ok_or_else(identity_error)?;
        let identifier = namespace
            .strip_prefix("urn:iso:std:iso:20022:tech:xsd:")
            .ok_or_else(identity_error)?;
        let version = identifier
            .strip_prefix(family)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .ok_or_else(identity_error)?;
        if !versions.contains(&version) {
            return Err(identity_error());
        }
        self.root.child(body).ok_or_else(identity_error)
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

pub(crate) fn is_xml_character(character: char) -> bool {
    matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x0001_0000..=0x0010_FFFF)
}

fn reference_text<'source>(
    reference: &quick_xml::events::BytesRef<'source>,
) -> Result<std::borrow::Cow<'source, str>, XmlError> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|error| XmlError::Malformed(error.to_string()))?
    {
        if !is_xml_character(character) {
            return Err(XmlError::Malformed(
                "invalid XML character reference".to_owned(),
            ));
        }
        return Ok(std::borrow::Cow::Owned(character.to_string()));
    }
    let raw = reference
        .decode()
        .map_err(|error| XmlError::Malformed(error.to_string()))?;
    quick_xml::escape::resolve_predefined_entity(&raw)
        .map(std::borrow::Cow::Borrowed)
        .ok_or_else(|| XmlError::UnknownEntity(raw.into_owned()))
}

fn check_declaration(
    declaration: &quick_xml::events::BytesDecl<'_>,
    ascii: bool,
) -> Result<(), XmlError> {
    if declaration
        .version()
        .map_err(|error| XmlError::Malformed(error.to_string()))?
        .as_ref()
        != b"1.0"
    {
        return Err(XmlError::Malformed("only XML 1.0 is supported".to_owned()));
    }
    let content = std::str::from_utf8(declaration.as_ref())
        .map_err(|error| XmlError::Malformed(error.to_string()))?;
    let fields = quick_xml::events::BytesStart::from_content(content, 3);
    let mut previous = 0;
    for attribute in fields.attributes() {
        let attribute = attribute.map_err(|error| XmlError::Malformed(error.to_string()))?;
        let rank = match attribute.key.as_ref() {
            b"version" => 1,
            b"encoding"
                if attribute.value.eq_ignore_ascii_case(b"UTF-8")
                    || (ascii && attribute.value.eq_ignore_ascii_case(b"US-ASCII")) =>
            {
                2
            }
            b"standalone" if matches!(attribute.value.as_ref(), b"yes" | b"no") => 3,
            _ => {
                return Err(XmlError::Malformed(
                    "invalid XML declaration field".to_owned(),
                ));
            }
        };
        if rank <= previous {
            return Err(XmlError::Malformed(
                "invalid XML declaration order".to_owned(),
            ));
        }
        previous = rank;
    }
    Ok(())
}

fn check_xml_text(text: &str) -> Result<(), XmlError> {
    if text.chars().all(is_xml_character) {
        Ok(())
    } else {
        Err(XmlError::Malformed("invalid XML character".to_owned()))
    }
}

/// Build a [`Node`] from a start/empty tag, stripping namespace prefixes.
fn resolved_namespace(
    result: &ResolveResult<'_>,
    namespaces: &mut HashMap<String, Arc<str>>,
) -> Result<Option<Arc<str>>, XmlError> {
    match result {
        ResolveResult::Unbound => Ok(None),
        ResolveResult::Unknown(_) => Err(XmlError::Malformed(
            "undeclared namespace prefix".to_owned(),
        )),
        ResolveResult::Bound(namespace) => {
            let raw_uri = std::str::from_utf8(namespace.as_ref())
                .map_err(|error| XmlError::Malformed(error.to_string()))?;
            let uri = quick_xml::escape::unescape(raw_uri)
                .map_err(|error| XmlError::Malformed(error.to_string()))?;
            if let Some(existing) = namespaces.get(uri.as_ref()) {
                return Ok(Some(Arc::clone(existing)));
            }
            let shared: Arc<str> = Arc::from(uri.as_ref());
            namespaces.insert(uri.into_owned(), Arc::clone(&shared));
            Ok(Some(shared))
        }
    }
}

fn element(
    e: &quick_xml::events::BytesStart<'_>,
    resolver: &NamespaceResolver,
    namespaces: &mut HashMap<String, Arc<str>>,
    budget: &mut Budget,
) -> Result<Node, XmlError> {
    let name = local_name(e.name().as_ref())?.to_owned();
    let namespace = resolved_namespace(&resolver.resolve_element(e.name()).0, namespaces)?;

    let mut attrs = Vec::new();
    let mut expanded = HashSet::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| XmlError::Malformed(err.to_string()))?;
        // `xmlns:ns2="…"` is recorded under its local name `ns2`; the default
        // declaration `xmlns="…"` keeps the key `xmlns`.
        let local = local_name(attr.key.as_ref())?.to_owned();
        let key = std::str::from_utf8(attr.key.as_ref())
            .map_err(|error| XmlError::Malformed(error.to_string()))?
            .to_owned();
        let attribute_namespace =
            resolved_namespace(&resolver.resolve_attribute(attr.key).0, namespaces)?;
        if !expanded.insert((attribute_namespace, local)) {
            return Err(XmlError::Malformed(
                "duplicate expanded attribute name".to_owned(),
            ));
        }
        check_limit("attributes", attrs.len() + 1, budget.limits.attributes)?;
        // Attribute-value normalisation per the XML 1.0 spec, resolving the
        // five predefined entities (so `Note="a &lt; b"` yields `a < b`).
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|err| XmlError::Malformed(err.to_string()))?
            .into_owned();
        check_xml_text(&value)?;
        check_limit(
            "attribute_bytes",
            value.len(),
            budget.limits.attribute_bytes,
        )?;
        charge(
            "total_attribute_bytes",
            &mut budget.attributes,
            value.len(),
            budget.limits.total_attribute_bytes,
        )?;
        attrs.push((key, value));
    }

    Ok(Node {
        namespace,
        name,
        attrs,
        ..Node::default()
    })
}

/// Strip an optional `prefix:` from a qualified name.
fn local_name(raw: &[u8]) -> Result<&str, XmlError> {
    let s = std::str::from_utf8(raw)
        .map_err(|_| XmlError::Malformed("element name is not valid UTF-8".to_owned()))?;
    let mut parts = s.split(':');
    let first = parts.next().unwrap_or_default();
    let local = parts.next();
    if !is_ncname(first) || local.is_some_and(|part| !is_ncname(part)) || parts.next().is_some() {
        return Err(XmlError::Malformed("invalid qualified XML name".to_owned()));
    }
    Ok(local.unwrap_or(first))
}

fn is_name_start(character: char) -> bool {
    matches!(character as u32, 0x41..=0x5A | 0x5F | 0x61..=0x7A |
        0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D |
        0x37F..=0x1FFF | 0x200C..=0x200D | 0x2070..=0x218F |
        0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF |
        0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

fn is_ncname(name: &str) -> bool {
    is_xml_name(name, false)
}

fn is_xml_name(name: &str, colon_allowed: bool) -> bool {
    let is_start = |character| is_name_start(character) || (colon_allowed && character == ':');
    let mut characters = name.chars();
    characters.next().is_some_and(is_start)
        && characters.all(|character| {
            is_start(character)
                || matches!(character as u32,
            0x2D | 0x2E | 0x30..=0x39 | 0xB7 | 0x300..=0x36F | 0x203F..=0x2040)
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
    fn overflow_and_hard_depth_are_not_unlimited_overrides() {
        let mut count = usize::MAX;
        assert!(charge("nodes", &mut count, 1, usize::MAX).is_err());
        assert_eq!(count, usize::MAX);
        let limits = ParseLimits {
            depth: MAX_DEPTH + 1,
            ..Default::default()
        };
        assert_eq!(
            Document::parse_with_limits("<R/>", limits),
            Err(XmlError::InvalidLimits)
        );
    }

    #[test]
    fn processing_instruction_targets_obey_xml_name_grammar() {
        for xml in ["<?1target?><R/>", "<R/><?target!?>", "<??><R/>"] {
            assert!(Document::parse(xml).is_err(), "invalid PI target: {xml}");
        }
        for xml in [
            "<?target data?><R/>",
            "<R/><?a:b:c data?>",
            "<?xml-stylesheet href='a'?><R/>",
        ] {
            assert!(Document::parse(xml).is_ok(), "lawful PI target: {xml}");
        }
    }

    #[test]
    fn every_resource_limit_accepts_its_boundary_and_refuses_one_over() {
        type Setter = fn(&mut ParseLimits, usize);
        let cases: &[(&str, &str, usize, Setter)] = &[
            ("input_bytes", "<R/>", 4, |limits, value| {
                limits.input_bytes = value;
            }),
            ("nodes", "<R><E/><E/></R>", 3, |limits, value| {
                limits.nodes = value;
            }),
            ("depth", "<R><E/></R>", 2, |limits, value| {
                limits.depth = value;
            }),
            (
                "text_bytes",
                "<R>a&#x62;<![CDATA[c]]></R>",
                3,
                |limits, value| limits.text_bytes = value,
            ),
            (
                "total_text_bytes",
                "<R>a<E>bc</E></R>",
                3,
                |limits, value| limits.total_text_bytes = value,
            ),
            ("attributes", "<R A='1' B='2'/>", 2, |limits, value| {
                limits.attributes = value;
            }),
            (
                "attribute_bytes",
                "<R A='a&#x62;c'/>",
                3,
                |limits, value| limits.attribute_bytes = value,
            ),
            (
                "total_attribute_bytes",
                "<R A='ab'><E B='cd'/></R>",
                4,
                |limits, value| limits.total_attribute_bytes = value,
            ),
        ];
        for &(resource, xml, count, set) in cases {
            let mut limits = ParseLimits::default();
            set(&mut limits, count);
            assert!(
                Document::parse_with_limits(xml, limits).is_ok(),
                "{resource}"
            );
            set(&mut limits, count - 1);
            assert!(
                matches!(Document::parse_with_limits(xml, limits), Err(XmlError::LimitExceeded { resource: actual, .. }) if actual == resource),
                "{resource}"
            );
            set(&mut limits, 0);
            assert_eq!(
                Document::parse_with_limits(xml, limits),
                Err(XmlError::InvalidLimits)
            );
        }
    }

    #[test]
    fn names_and_decoded_namespace_aliases_follow_xml_identity() {
        for xml in [
            "<1R/>",
            "<R? />",
            "<R a:b:c='1'/>",
            "<R xmlns:a='urn:x' xmlns:b='urn&#58;x' a:Id='1' b:Id='2'/>",
        ] {
            assert!(Document::parse(xml).is_err(), "{xml}");
        }
        let document = Document::parse("<R xmlns='urn&#58;x'><E xmlns='urn:x'/></R>").unwrap();
        assert!(document.root.child("E").is_some());
        assert_eq!(parse("<R> a\r\nb\rc </R>").text, " a\nb\nc ");
    }

    #[test]
    fn broad_documents_have_a_finite_node_budget() {
        let xml = format!("<R>{}</R>", "<E/>".repeat(250_000));
        assert!(Document::parse(&xml).is_err());
    }

    #[test]
    fn namespace_identity_is_resolved_not_inferred_from_prefix_spelling() {
        let document = Document::parse("<p:R xmlns:p=\"urn:x\"><p:Amt>1</p:Amt></p:R>").unwrap();
        assert_eq!(document.namespace.as_deref(), Some("urn:x"));
        assert!(Document::parse("<p:R/>").is_err());
        assert!(
            Document::parse("<R xmlns:a=\"urn:x\" xmlns:b=\"urn:x\" a:Id=\"1\" b:Id=\"2\"/>")
                .is_err()
        );
        let document =
            Document::parse("<R xmlns=\"urn:x\"><Amt xmlns=\"urn:other\">2</Amt></R>").unwrap();
        assert!(document.root.child("Amt").is_none());
    }

    #[test]
    fn malformed_envelopes_are_refused() {
        for xml in [
            "junk<R/>",
            "<R/>junk",
            "<R/>&unknown;",
            "<![CDATA[before]]><R/>",
            "<R/><unfinished>",
            "<R>",
            "<R/><?xml version=\"1.0\"?>",
            "<!-- invalid -- comment --><R/>",
            "<R>&#0;</R>",
        ] {
            assert!(Document::parse(xml).is_err(), "accepted {xml:?}");
        }
        assert!(Document::parse(" \n<!--ok--><?bank ok?><R/>\n<!--ok-->").is_ok());
    }

    #[test]
    fn empty_elements_obey_the_same_depth_limit() {
        let open = "<R>".repeat(MAX_DEPTH);
        let close = "</R>".repeat(MAX_DEPTH);
        assert!(Document::parse(&format!("{open}<R/>{close}")).is_err());
        assert!(Document::parse(&format!("{open}{close}")).is_ok());
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
    fn a_self_closing_root_cannot_be_displaced_either() {
        // Regression: `Event::Empty` at depth 0 assigned the root directly,
        // bypassing the duplicate check — so `<Doc/>` followed by a second
        // root silently handed the caller the *second* document.
        assert_eq!(
            Document::parse("<Doc/><Doc xmlns='urn:evil'><MsgId>EVIL</MsgId></Doc>"),
            Err(XmlError::MultipleRootElements)
        );
        assert_eq!(
            Document::parse("<Doc><MsgId>REAL</MsgId></Doc><Doc/>"),
            Err(XmlError::MultipleRootElements)
        );
        // A document that really is just a self-closing root still parses.
        assert_eq!(Document::parse("<Doc/>").unwrap().root.name, "Doc");
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

        // `GenericIdentification3` — the shape `ChargeType3Choice/Prtry` has,
        // where `Id` is mandatory and the element itself carries no text.
        let generic = parse("<Tp><Prtry><Id>RETURN_FEE</Id></Prtry></Tp>");
        assert_eq!(generic.code(), Some("RETURN_FEE"));

        // `ProprietaryBankTransactionCodeStructure1` — `Cd` inside `Prtry`.
        let bank_tx = parse("<BkTxCd><Prtry><Cd>NTRF+001</Cd><Issr>DK</Issr></Prtry></BkTxCd>");
        assert_eq!(bank_tx.code(), Some("NTRF+001"));

        // A `Cd` at the top still wins over anything inside `Prtry`.
        let both = parse("<Tp><Cd>CHRG</Cd><Prtry><Id>IGNORED</Id></Prtry></Tp>");
        assert_eq!(both.code(), Some("CHRG"));
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
        let doc =
            Document::parse(r#"<n:Document xmlns:n="urn:iso:y"><n:A/></n:Document>"#).unwrap();
        assert_eq!(doc.namespace.as_deref(), Some("urn:iso:y"));
        assert_eq!(doc.root.attr("xmlns:n"), Some("urn:iso:y"));
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
        assert_eq!(parse("<Nm>  Max &amp; Co  </Nm>").text, "  Max & Co  ");
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
