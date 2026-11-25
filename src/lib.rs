//! Read, write and validate [DiveJSON](https://divejson.org) 1.0, the open dive-log
//! interchange format.
//!
//! [`Document`] is a typed model of the whole format. Reading is strict: [`from_str`] and
//! its siblings reject anything that is not a conforming 1.0 document — malformed JSON, a
//! duplicate member name, `null`, an undefined member outside `extensions`, an unknown
//! vocabulary value, a malformed uuid or date-time — and then run [`Document::validate`],
//! which checks every remaining schema constraint and every rule spec §3 lists as living
//! outside the schema. Writing validates first, so [`to_string`] and its siblings only ever
//! emit conforming documents.
//!
//! ```
//! let text = r#"{"format": "divejson", "version": "1.0", "exported_at": "2026-04-17T11:49:23+02:00"}"#;
//! let doc = divejson::from_str(text)?;
//! assert!(doc.dives.is_empty());
//! let again = divejson::from_str(&divejson::to_string(&doc)?)?;
//! assert_eq!(doc, again);
//! # Ok::<(), divejson::Error>(())
//! ```
//!
//! Units follow spec §5.1 and are documented on each field: meters, degrees Celsius, bar,
//! liters, percent, seconds; profile channels are integers on a millisecond axis, depth in
//! centimeters, temperature in hundredths of a degree, pressure in tenths of a bar.
//!
//! # Features
//! - `std` (default): [`from_reader`] and [`to_writer`]. Without it the crate is `no_std`
//!   with `alloc`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod dive;
mod document;
mod error;
mod read;
mod recording;
mod records;
#[cfg(feature = "uddf")]
pub mod uddf;
mod validate;
mod values;

use alloc::string::String;
use alloc::vec::Vec;

use serde::de::DeserializeSeed as _;

pub use jiff;
pub use serde_json;
pub use uuid;

pub use dive::*;
pub use document::*;
pub use error::Error;
pub use recording::*;
pub use records::*;
pub use validate::{Code, Violation, folded};
pub use values::{Date, DateTime, StartedAt, Uuid, ValueError};

/// The specification version this crate implements.
pub const SPEC_VERSION: &str = "1.0";

/// The media type of a DiveJSON document (§8).
pub const MEDIA_TYPE: &str = "application/vnd.dive+json";

/// Read and validate a document from JSON text.
///
/// # Errors
/// [`Error::Json`] when the text is not a well-formed DiveJSON 1.0 document,
/// [`Error::Invalid`] when it breaks a rule.
pub fn from_str(text: &str) -> Result<Document, Error> {
    from_slice(text.as_bytes())
}

/// Read and validate a document from UTF-8 JSON bytes.
///
/// # Errors
/// As [`from_str`].
pub fn from_slice(bytes: &[u8]) -> Result<Document, Error> {
    read::Strict {
        in_extensions: false,
    }
    .deserialize(&mut serde_json::Deserializer::from_slice(bytes))?;
    let doc: Document = serde_json::from_slice(bytes)?;
    doc.validate().map_err(Error::Invalid)?;
    Ok(doc)
}

/// Read and validate a document from a reader. The whole input is buffered first.
///
/// # Errors
/// As [`from_str`], plus [`Error::Io`].
#[cfg(feature = "std")]
pub fn from_reader(mut reader: impl std::io::Read) -> Result<Document, Error> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    from_slice(&bytes)
}

/// Validate and write a document as compact JSON.
///
/// # Errors
/// [`Error::Invalid`] when the document breaks a rule.
pub fn to_string(doc: &Document) -> Result<String, Error> {
    doc.validate().map_err(Error::Invalid)?;
    Ok(serde_json::to_string(doc)?)
}

/// Validate and write a document as indented JSON.
///
/// # Errors
/// As [`to_string`].
pub fn to_string_pretty(doc: &Document) -> Result<String, Error> {
    doc.validate().map_err(Error::Invalid)?;
    Ok(serde_json::to_string_pretty(doc)?)
}

/// Validate and write a document as compact JSON to a writer.
///
/// # Errors
/// As [`to_string`], plus [`Error::Json`] wrapping a write failure.
#[cfg(feature = "std")]
pub fn to_writer(writer: impl std::io::Write, doc: &Document) -> Result<(), Error> {
    doc.validate().map_err(Error::Invalid)?;
    Ok(serde_json::to_writer(writer, doc)?)
}

/// Validate and write a document as UTF-8 JSON bytes.
///
/// # Errors
/// As [`to_string`].
pub fn to_vec(doc: &Document) -> Result<Vec<u8>, Error> {
    doc.validate().map_err(Error::Invalid)?;
    Ok(serde_json::to_vec(doc)?)
}
