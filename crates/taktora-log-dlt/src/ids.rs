//! 4-character DLT identifiers (REQ_0808).

use thiserror::Error;

/// Errors returned by [`AppId::new`] / [`CtxId::new`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    /// The id was not exactly 4 bytes long.
    #[error("DLT id must be exactly 4 characters, got {len}")]
    WrongLength {
        /// observed length in bytes
        len: usize,
    },
    /// The id contained non-ASCII bytes.
    #[error("DLT id must be ASCII")]
    NonAscii,
}

macro_rules! four_char_id {
    ($name:ident, $purpose:literal) => {
        #[doc = concat!("4-character DLT ", $purpose, " identifier.")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Construct an id from a 4-character ASCII string.
            pub fn new(s: &str) -> Result<Self, IdError> {
                if s.len() != 4 {
                    return Err(IdError::WrongLength { len: s.len() });
                }
                if !s.is_ascii() {
                    return Err(IdError::NonAscii);
                }
                Ok(Self(s.to_string()))
            }

            /// Borrow the id as a `&str`.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

four_char_id!(AppId, "application");
four_char_id!(CtxId, "context");
