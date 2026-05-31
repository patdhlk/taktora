//! Error type and source positions for ESI parsing.

use thiserror::Error;

/// A 1-based source position within the ESI XML document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number.
    pub column: u32,
}

/// Errors that can occur while parsing an ESI file.
#[derive(Debug, Error)]
pub enum EsiError {
    /// The XML was syntactically invalid.
    #[error("ESI XML syntax error at {span:?}: {source}")]
    Xml {
        /// Source position of the failure, when recoverable.
        span: Span,
        /// Underlying quick-xml deserialization error.
        source: quick_xml::DeError,
    },
    /// The document deserialized but a value field was invalid.
    #[error("invalid ESI value at `{path}`{}: {reason}", .span.map_or(String::new(), |s| format!(" ({s:?})")))]
    Value {
        /// Dotted element/attribute path to the offending value.
        path: String,
        /// Source position, when recoverable.
        span: Option<Span>,
        /// Human-readable reason.
        reason: String,
    },
    /// An ESI integer field (decimal or `#x`-prefixed hex) could not be parsed.
    #[error("invalid ESI integer `{raw}` at `{path}`")]
    Number {
        /// The raw text that failed to parse.
        raw: String,
        /// Dotted path to the offending value.
        path: String,
    },
}
