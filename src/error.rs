//! The crate's error type.

use alloc::string::ToString;
use alloc::vec::Vec;

use crate::Violation;

/// Why a document could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The text is not JSON, or not shaped like a DiveJSON 1.0 document: a wrong type, a
    /// missing required member, an undefined member, an unknown vocabulary value, `null`, a
    /// duplicate member name, or a malformed uuid, date or date-time.
    #[error("not a DiveJSON 1.0 document: {0}")]
    Json(#[from] serde_json::Error),
    /// The document is well-formed but breaks one or more rules (§3); every one is listed.
    #[error("{} DiveJSON rule violation(s), first: {}", .0.len(), .0.first().map_or_else(Default::default, ToString::to_string))]
    Invalid(Vec<Violation>),
    /// Reading the input failed.
    #[cfg(feature = "std")]
    #[error("reading the document failed: {0}")]
    Io(#[from] std::io::Error),
    /// The input is not UDDF: malformed XML, a `<!DOCTYPE>`, or a root other than `<uddf>`.
    #[cfg(feature = "uddf")]
    #[error("not a UDDF document: {0}")]
    Uddf(#[from] ::uddf::Error),
}

impl Error {
    /// The rule violations, when that is what the error is.
    #[must_use]
    pub fn violations(&self) -> &[Violation] {
        match self {
            Self::Invalid(violations) => violations,
            _ => &[],
        }
    }
}
