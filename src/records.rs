//! The top-level records other than dives: trips (§6.8–6.9), sites (§6.10), species
//! (§6.11), gear (§6.12–6.15), certifications and courses (§6.16–6.17), contacts
//! (§6.18–6.19) and people (§6.20).

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{
    BoundingBox, Date, DateTime, EntryType, Extensions, PersonReference, Position, StoredFile,
    Uuid, WaterType,
};

/// A trip: a sequence of parts (§6.8). Its dives link to it, not the other way round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Trip {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// The stretches of the trip, in the diver's own order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<TripPart>,
    /// Who came on the trip, whether or not they dived.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonReference>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Trip {
    /// A trip with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            parts: Vec::new(),
            people: Vec::new(),
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// One stretch of a trip (§6.9a); every member optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct TripPart {
    /// First day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_on: Option<Date>,
    /// Last day; ≥ `starts_on`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_on: Option<Date>,
    /// Where this stretch was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// → `contacts`: where the diver slept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accommodation_uuid: Option<Uuid>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// A named place (§6.9). `bbox` requires `position`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Location {
    /// The place as a person writes it, 1–255.
    pub name: String,
    /// Where the place is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    /// The geocoded extent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BoundingBox>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Location {
    /// A place known by name only.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            position: None,
            bbox: None,
            extensions: Extensions::new(),
        }
    }
}

/// A dive site (§6.10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DiveSite {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// Other names, each 1–255, none repeating another or `name` once trimmed and
    /// case-folded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<String>,
    /// The locality the site is in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
    /// Where the site itself is; a different fact from `location.position`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    /// Entries in outside registries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_ids: Vec<ExternalId>,
    /// Meters; ≥ 0, the shallowest depth dived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_from: Option<f64>,
    /// Meters; ≥ `depth_from`, the deepest depth dived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_to: Option<f64>,
    /// The water at the site.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_type: Option<WaterType>,
    /// Meters above sea level; −450 to 6500.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude: Option<i32>,
    /// Every way divers enter the water there, no value twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_types: Vec<EntryType>,
    /// The diver's labels, under a dive's tag rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl DiveSite {
    /// A site with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            other_names: Vec::new(),
            location: None,
            position: None,
            external_ids: Vec::new(),
            depth_from: None,
            depth_to: None,
            water_type: None,
            altitude: None,
            entry_types: Vec::new(),
            tags: Vec::new(),
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A record's entry in an outside registry (§6.10).
///
/// `registry` is a producer key; `wikidata` (`Q…`), `openstreetmap` (`node|way|relation/…`)
/// and `worms` (AphiaID digits) hold `identifier` to their form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExternalId {
    /// The registry, `^[a-z0-9][a-z0-9._-]*$`.
    pub registry: String,
    /// What the registry calls the record, 1–255.
    pub identifier: String,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ExternalId {
    /// An entry from its registry and identifier.
    #[must_use]
    pub fn new(registry: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            registry: registry.into(),
            identifier: identifier.into(),
            extensions: Extensions::new(),
        }
    }
}

/// A species the diver has sighted (§6.11). Needs a name or an external id; at most one
/// `worms` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Species {
    /// Unique across the document.
    pub uuid: Uuid,
    /// Entries in outside registries; the `worms` one is the matching key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_ids: Vec<ExternalId>,
    /// 1–255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scientific_name: Option<String>,
    /// 1–255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_name: Option<String>,
    /// WoRMS's open rank vocabulary, ≤ 64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Species {
    /// A species with only its uuid; give it a name or an external id before writing.
    #[must_use]
    pub fn new(uuid: Uuid) -> Self {
        Self {
            uuid,
            external_ids: Vec::new(),
            scientific_name: None,
            common_name: None,
            rank: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A piece of the diver's kit (§6.12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GearItem {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// The maker, ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    /// The maker's serial, 1–64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// What kind of kit it is.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub gear_type: Option<GearType>,
    /// Liters of water capacity; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    /// Bar; > 0 and ≤ 350.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_pressure: Option<f64>,
    /// The vessel's material.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<Material>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Rented rather than owned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rented: Option<bool>,
    /// Retired from active use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    /// When it was retired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime>,
    /// Derived: dives that used it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_count: Option<u64>,
    /// Derived: seconds of those dives' `submerged_time`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_time: Option<u64>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl GearItem {
    /// A gear item with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            brand: None,
            serial: None,
            gear_type: None,
            volume: None,
            working_pressure: None,
            material: None,
            notes: None,
            rented: None,
            archived: None,
            archived_at: None,
            dive_count: None,
            dive_time: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A gear item's `type` (§6.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum GearType {
    Mask,
    Snorkel,
    Fins,
    Wetsuit,
    Drysuit,
    Vest,
    Hood,
    Gloves,
    Boots,
    /// A buoyancy compensator recorded whole.
    Bcd,
    /// A plate with its harness.
    Backplate,
    /// A bladder recorded apart from its plate.
    Wing,
    Regulator,
    Rebreather,
    Computer,
    /// A tank-pressure pod.
    Transmitter,
    Cylinder,
    Light,
    /// A diver propulsion vehicle.
    Scooter,
    Smb,
    Mirror,
    Whistle,
    Reel,
    Knife,
    LineCutter,
    Shears,
    Compass,
    Camera,
    Other,
}

/// A vessel's material (§6.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Material {
    /// Steel.
    Steel,
    /// Aluminium.
    Aluminium,
    /// Carbon composite.
    Carbon,
}

/// A named bundle of gear (§6.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GearSet {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// Kilograms of ballast; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// → `gear`, in the set's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear_uuids: Vec<Uuid>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl GearSet {
    /// A gear set with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            weight: None,
            gear_uuids: Vec::new(),
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A service schedule's or record's `type` (§6.14). A required member's vocabulary, so it
/// does not grow in minor versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceType {
    /// A service.
    Service,
    /// A visual inspection.
    VisualInspection,
    /// A hydrostatic test.
    HydrostaticTest,
    /// A battery change.
    Battery,
    /// An oxygen clean.
    OxygenClean,
    /// Anything else.
    Other,
}

/// A recurring maintenance rule (§6.14). At least one interval is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ServiceSchedule {
    /// Unique across the document.
    pub uuid: Uuid,
    /// → `gear`.
    pub gear_uuid: Uuid,
    /// What kind of service.
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    /// ≤ 120.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// When the rule's clock starts.
    pub starts_on: Date,
    /// Months; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_months: Option<u64>,
    /// Dives; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_dives: Option<u64>,
    /// Hours of diving; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_hours: Option<u64>,
    /// Snapshot: the item's dive count when the rule started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_count_at_start: Option<u64>,
    /// Snapshot: the item's dive time in seconds when the rule started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_time_at_start: Option<u64>,
    /// Whether the rule is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Derived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_service_on: Option<Date>,
    /// Derived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_due_on: Option<Date>,
    /// Derived: the lifetime dive count at which service falls due.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_due_at_dive_count: Option<u64>,
    /// Derived: the dive time in seconds at which service falls due.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_due_at_dive_time: Option<u64>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ServiceSchedule {
    /// A schedule with its required members; set at least one interval before writing.
    #[must_use]
    pub fn new(uuid: Uuid, gear_uuid: Uuid, service_type: ServiceType, starts_on: Date) -> Self {
        Self {
            uuid,
            gear_uuid,
            service_type,
            label: None,
            starts_on,
            interval_months: None,
            interval_dives: None,
            interval_hours: None,
            dive_count_at_start: None,
            dive_time_at_start: None,
            active: None,
            last_service_on: None,
            next_due_on: None,
            next_due_at_dive_count: None,
            next_due_at_dive_time: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// One performed maintenance event (§6.15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ServiceRecord {
    /// Unique across the document.
    pub uuid: Uuid,
    /// → `gear`.
    pub gear_uuid: Uuid,
    /// → `gear_service_schedules`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear_service_schedule_uuid: Option<Uuid>,
    /// What kind of service.
    #[serde(rename = "type")]
    pub service_type: ServiceType,
    /// When it was done.
    pub serviced_on: Date,
    /// Snapshot: the item's dive count at the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_count_at_service: Option<u64>,
    /// Snapshot: the item's dive time in seconds at the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_time_at_service: Option<u64>,
    /// ≤ 120.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Who did the work, ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performed_by: Option<String>,
    /// → `contacts`: where it was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_uuid: Option<Uuid>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl ServiceRecord {
    /// A service record with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, gear_uuid: Uuid, service_type: ServiceType, serviced_on: Date) -> Self {
        Self {
            uuid,
            gear_uuid,
            gear_service_schedule_uuid: None,
            service_type,
            serviced_on,
            dive_count_at_service: None,
            dive_time_at_service: None,
            label: None,
            performed_by: None,
            contact_uuid: None,
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A training agency (§6.16). Frozen after 1.0: an agency not listed is `Other`, named in
/// `agency_other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum Agency {
    Padi,
    Ssi,
    Naui,
    Sdi,
    Tdi,
    Cmas,
    Raid,
    Bsac,
    Gue,
    Iantd,
    Psai,
    Dan,
    Efr,
    Andi,
    Snsi,
    Acuc,
    Pss,
    Ida,
    Ndl,
    Utd,
    Saa,
    Scotsac,
    Iac,
    Protec,
    Pdic,
    Nase,
    Sei,
    Ymca,
    Erdi,
    Aida,
    Molchanovs,
    Pfi,
    ApneaAcademy,
    Fii,
    NssCds,
    Nacd,
    Idea,
    Diwa,
    /// Named in `agency_other`.
    Other,
}

/// A certification card (§6.16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Certification {
    /// Unique across the document.
    pub uuid: Uuid,
    /// The issuing agency.
    pub agency: Agency,
    /// ≤ 64; present exactly when `agency` is `Other`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency_other: Option<String>,
    /// 1–255.
    pub name: String,
    /// ≤ 64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// Issue date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certified_on: Option<Date>,
    /// Expiry date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_on: Option<Date>,
    /// → `people`: the certifying instructor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructor_uuid: Option<Uuid>,
    /// ≤ 64, as printed on the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructor_number: Option<String>,
    /// → `contacts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_uuid: Option<Uuid>,
    /// → `courses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_uuid: Option<Uuid>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Scan of the card's front.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_file: Option<StoredFile>,
    /// Scan of the card's back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub back_file: Option<StoredFile>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Certification {
    /// A certification with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, agency: Agency, name: impl Into<String>) -> Self {
        Self {
            uuid,
            agency,
            agency_other: None,
            name: name.into(),
            number: None,
            certified_on: None,
            expires_on: None,
            instructor_uuid: None,
            instructor_number: None,
            contact_uuid: None,
            course_uuid: None,
            notes: None,
            front_file: None,
            back_file: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A training course (§6.17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Course {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// Absent for a course that names no agency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency: Option<Agency>,
    /// ≤ 64; present exactly when `agency` is `Other`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agency_other: Option<String>,
    /// Where the course stands; absent is not "completed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<CourseStatus>,
    /// First day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_on: Option<Date>,
    /// Last day; ≥ `starts_on`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ends_on: Option<Date>,
    /// ≤ 64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructor_number: Option<String>,
    /// → `contacts`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_uuid: Option<Uuid>,
    /// Instructors and fellow students.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonReference>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Course {
    /// A course with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            agency: None,
            agency_other: None,
            status: None,
            starts_on: None,
            ends_on: None,
            instructor_number: None,
            contact_uuid: None,
            people: Vec::new(),
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A course's `status` (§6.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CourseStatus {
    /// Booked.
    Planned,
    /// Under way.
    InProgress,
    /// Completed.
    Completed,
    /// Left incomplete.
    Incomplete,
    /// A provisional pass.
    Provisional,
    /// Not passed.
    NotPassed,
}

/// A party the diver dealt with: a business, club or household (§6.18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Contact {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255.
    pub name: String,
    /// What the contact is, no value twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<ContactRole>,
    /// ≤ 32, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// An absolute URL, ≤ 512.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// Postal address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// Where it is on a map; never derived from `address`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Contact {
    /// A contact with its required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            roles: Vec::new(),
            phone: None,
            email: None,
            website: None,
            address: None,
            position: None,
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A contact's role (§6.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContactRole {
    /// Runs dives and day boats.
    DiveCenter,
    /// Teaches courses.
    School,
    /// Sells or services equipment.
    Shop,
    /// Has rooms.
    Accommodation,
    /// Operates boats a diver sleeps on.
    Liveaboard,
    /// A membership association.
    Club,
    /// None of these.
    Other,
}

/// A postal address (§6.19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Address {
    /// ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub street: Option<String>,
    /// ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// ≤ 32.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postcode: Option<String>,
    /// ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// 1–255; the one member an address cannot omit.
    pub country: String,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Address {
    /// An address in a country.
    #[must_use]
    pub fn new(country: impl Into<String>) -> Self {
        Self {
            street: None,
            city: None,
            postcode: None,
            region: None,
            country: country.into(),
            extensions: Extensions::new(),
        }
    }
}

/// An individual the diver was with (§6.20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Person {
    /// Unique across the document.
    pub uuid: Uuid,
    /// 1–255, as the diver writes it.
    pub name: String,
    /// ≤ 255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// ≤ 32, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Person {
    /// A person with their required members.
    #[must_use]
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            email: None,
            phone: None,
            notes: None,
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}
