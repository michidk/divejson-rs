//! The envelope (§4), the diver (§6.1) and the small objects several records share.

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    Certification, Contact, Course, DateTime, Dive, DiveSite, GearItem, GearSet, Person,
    ServiceRecord, ServiceSchedule, Species, Trip, Uuid,
};

/// Producer-keyed extension data (§5.5): member names match `^[a-z0-9][a-z0-9._-]*$`,
/// values are any JSON. An empty map is not written.
pub type Extensions = serde_json::Map<String, serde_json::Value>;

/// The `format` member: always the literal `"divejson"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Format {
    /// `"divejson"`.
    #[default]
    #[serde(rename = "divejson")]
    DiveJson,
}

/// The `version` member: the specification version a document conforms to.
///
/// Only `"1.0"` is accepted; this crate reads documents strictly against the 1.0 schema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Version {
    /// `"1.0"`.
    #[default]
    #[serde(rename = "1.0")]
    V1_0,
}

/// A DiveJSON document (§4).
///
/// Every collection is a `Vec`; an absent collection reads as empty and an empty one is not
/// written, which §4 makes equivalent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Document {
    /// Always `"divejson"`.
    pub format: Format,
    /// The specification version, `"1.0"`.
    pub version: Version,
    /// When the document was produced. Must carry a UTC offset (§5.2).
    pub exported_at: DateTime,
    /// What produced the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
    /// Whose logbook this is; absent when the source records nothing about its owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diver: Option<Diver>,
    /// Dives, chronologically by convention.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dives: Vec<Dive>,
    /// Trips.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trips: Vec<Trip>,
    /// Training courses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub courses: Vec<Course>,
    /// Dive sites.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<DiveSite>,
    /// Species the dives' sightings reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub species: Vec<Species>,
    /// Gear items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear: Vec<GearItem>,
    /// Gear sets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear_sets: Vec<GearSet>,
    /// Service schedules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear_service_schedules: Vec<ServiceSchedule>,
    /// Service records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear_service_records: Vec<ServiceRecord>,
    /// Certifications.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub certifications: Vec<Certification>,
    /// Contacts: businesses, clubs and households.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<Contact>,
    /// People the diver was with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<Person>,
    /// Producer extensions (§5.5).
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Document {
    /// An empty logbook exported at `exported_at`.
    #[must_use]
    pub fn new(exported_at: DateTime) -> Self {
        Self {
            format: Format::DiveJson,
            version: Version::V1_0,
            exported_at,
            generator: None,
            diver: None,
            dives: Vec::new(),
            trips: Vec::new(),
            courses: Vec::new(),
            sites: Vec::new(),
            species: Vec::new(),
            gear: Vec::new(),
            gear_sets: Vec::new(),
            gear_service_schedules: Vec::new(),
            gear_service_records: Vec::new(),
            certifications: Vec::new(),
            contacts: Vec::new(),
            people: Vec::new(),
            extensions: Extensions::new(),
        }
    }
}

/// What produced a document (§4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Generator {
    /// The producing software; at least one character.
    pub name: String,
    /// Its release, free-form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Generator {
    /// A generator with a name and nothing else.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
            extensions: Extensions::new(),
        }
    }
}

/// The owner of the logbook (§6.1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Diver {
    /// Stable across the same source's exports; never minted per export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<Uuid>,
    /// Display name, ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Contact address, ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Phone number as written, ≤ 32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Date of birth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub born_on: Option<crate::Date>,
    /// Who to call, in the order they are to be called.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emergency_contacts: Vec<EmergencyContact>,
    /// Dive insurance policies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub insurances: Vec<Insurance>,
    /// A photograph identifying the diver (§6.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portrait_file: Option<StoredFile>,
    /// When the account or logbook was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// A person to call if something happens to the diver (§6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct EmergencyContact {
    /// 1–255.
    pub name: String,
    /// ≤ 32, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// ≤ 64, free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl EmergencyContact {
    /// An emergency contact with a name and nothing else.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            phone: None,
            relationship: None,
            extensions: Extensions::new(),
        }
    }
}

/// One dive insurance policy (§6.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Insurance {
    /// The insurer, 1–255.
    pub provider: String,
    /// Policy or member number, ≤ 64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// The last day of cover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_on: Option<crate::Date>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Insurance {
    /// An insurance with a provider and nothing else.
    #[must_use]
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            number: None,
            expires_on: None,
            extensions: Extensions::new(),
        }
    }
}

/// A WGS 84 point in decimal degrees (§6). Half a coordinate is unrepresentable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Position {
    /// −90 to 90.
    pub latitude: f64,
    /// −180 to 180.
    pub longitude: f64,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Position {
    /// A position from latitude and longitude.
    #[must_use]
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
            extensions: Extensions::new(),
        }
    }
}

/// A geocoded extent (§6.9); `west > east` crosses the antimeridian.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct BoundingBox {
    /// −90 to 90, ≤ `north`.
    pub south: f64,
    /// −90 to 90.
    pub north: f64,
    /// −180 to 180.
    pub west: f64,
    /// −180 to 180.
    pub east: f64,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl BoundingBox {
    /// A box from its four edges.
    #[must_use]
    pub fn new(south: f64, north: f64, west: f64, east: f64) -> Self {
        Self {
            south,
            north,
            west,
            east,
            extensions: Extensions::new(),
        }
    }
}

/// Metadata for a binary the source logbook stores (§6.7); the bytes are not in the
/// document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct StoredFile {
    /// Unique across the whole document.
    pub uuid: Uuid,
    /// As uploaded, 1–255.
    pub original_filename: String,
    /// The stored media type, 1–64.
    pub content_type: String,
    /// Size in bytes.
    pub byte_size: u64,
    /// Lowercase hex SHA-256 of the stored bytes.
    pub sha256: String,
    /// The container-relative path inside an archive; absent in a bare document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_path: Option<String>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl StoredFile {
    /// A stored file from its required members.
    #[must_use]
    pub fn new(
        uuid: Uuid,
        original_filename: impl Into<String>,
        content_type: impl Into<String>,
        byte_size: u64,
        sha256: impl Into<String>,
    ) -> Self {
        Self {
            uuid,
            original_filename: original_filename.into(),
            content_type: content_type.into(),
            byte_size,
            sha256: sha256.into(),
            archive_path: None,
            extensions: Extensions::new(),
        }
    }
}
