//! UDDF in both directions: [`read`] converts a UDDF file into a DiveJSON document and
//! [`write()`] writes a DiveJSON document as UDDF, each with a [`Report`] of what the other
//! format could not carry.
//!
//! The rules are the vendored `docs/uddf-mapping.md` (reading), `docs/uddf-writing.md` and
//! `docs/writing.md` (writing) and `docs/converting.md` (both). Parsing and serialising the
//! XML is the `uddf` crate's.
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let text = r#"<uddf version="3.2.2"><profiledata><repetitiongroup><dive id="d1">
//!   <informationbeforedive><datetime>2026-04-19T08:12:00+02:00</datetime></informationbeforedive>
//!   <informationafterdive><greatestdepth>18.4</greatestdepth></informationafterdive>
//! </dive></repetitiongroup></profiledata></uddf>"#;
//! let exported_at = "2026-09-05T00:00:00+00:00".parse()?;
//! let (doc, report) = divejson::uddf::read(text, exported_at)?;
//! assert_eq!(doc.dives[0].max_depth, Some(18.4));
//! assert!(report.iter().all(|note| note.kind != divejson::uddf::NoteKind::Inferred));
//!
//! let (xml, _report) = divejson::uddf::write(&doc)?;
//! assert!(xml.contains("<greatestdepth>18.4</greatestdepth>"));
//! # Ok(()) }
//! ```

mod read;
mod write;

use alloc::string::String;
use alloc::vec::Vec;

pub use read::read;
pub use write::write;

/// What a [`Note`] says about a value (`docs/converting.md`, *A finding has a kind*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NoteKind {
    /// The source never recorded this.
    Absent,
    /// The converter computed this from readings the source did record.
    Inferred,
    /// The source recorded the value and left its scale, units or meaning open, and the
    /// converter decided how to read it; or it was read onto a coarser scale.
    Resolved,
    /// The source recorded this and the target format could not carry it.
    Dropped,
}

/// One finding of a conversion's report.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Note {
    /// What the finding says.
    pub kind: NoteKind,
    /// The stable code of the habit it reports, such as `start_time_zone_missing`. Reading
    /// notes carry one; writing notes do not.
    pub code: Option<&'static str>,
    /// Where it is: a path into the **source** — `dive/0/tankdata/1`, `site/3`, `$` for the
    /// file — when reading UDDF, and into the document being written when writing it.
    pub at: String,
    /// The DiveJSON member the finding is about, such as `dives/0/submerged_time`, where a
    /// reading note names one.
    pub member: Option<String>,
    /// The finding, in words.
    pub message: String,
}

/// Everything a conversion could not carry or had to decide, in the order it was found.
pub type Report = Vec<Note>;

/// `uuid5(NAMESPACE_URL, "https://divejson.org/ns/uddf")`: the identity namespace UDDF
/// records without a UUID of their own are derived under (`docs/uddf-mapping.md`,
/// *Identity*). Fixed forever.
pub const ID_NAMESPACE: uuid::Uuid = uuid::uuid!("1b85a949-d5f7-5d67-9d04-dcc78342f907");

/// Kelvin at 0 °C.
const KELVIN_OFFSET: f64 = 273.15;
/// Pascal per bar.
const PASCAL_PER_BAR: f64 = 100_000.0;
/// Litres per cubic metre.
const LITRES_PER_CUBIC_METRE: f64 = 1000.0;

/// A product of decimal factors as the decimal it stands for: the binary error of
/// `297.55 - 273.15` or `1.013 * 100000` rounded off at nine decimals, which is what lets a
/// value land on the number a decimal implementation computes.
fn tidy(value: f64) -> f64 {
    format!("{value:.9}").parse().unwrap_or(value)
}

/// The nearest integer, halves away from zero, or `None` where there is none.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "range-checked just above; f64 has no checked conversion to an integer"
)]
fn rounded(value: f64) -> Option<i64> {
    let value = tidy(value).round();
    (value.is_finite() && value.abs() < 9.0e15).then_some(value as i64)
}

/// UDDF's `<divetime>` seconds as a place on the millisecond axis, or the reverse.
fn milliseconds(seconds: f64) -> Option<i64> {
    rounded(seconds * 1000.0)
}

/// A place on the axis as a report speaks it, in seconds: `1200.02`, `30`.
#[allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "an axis instant is far below 2^53"
)]
fn in_seconds(milliseconds: i64) -> String {
    format!("{}", tidy(milliseconds as f64 / 1000.0))
}
