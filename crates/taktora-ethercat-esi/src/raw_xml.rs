//! Opaque capture of unrecognised vendor-extension XML.

/// An opaque XML element preserved verbatim (name, attributes, text, children).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawXml {
    /// Element local name.
    pub name: String,
    /// Attributes as `(name, value)` pairs, in document order.
    pub attributes: Vec<(String, String)>,
    /// Direct text content, when present.
    pub text: Option<String>,
    /// Child elements, recursively.
    pub children: Vec<Self>,
}
