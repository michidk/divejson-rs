//! A dive (§6.2) and what is embedded in it: sightings (§6.3a), cylinders (§6.3) and
//! person references (§6.20). Recordings are in their own module.

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DateTime, Extensions, Position, Recording, StartedAt, Uuid};

/// One dive: the diver's logbook entry (§6.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Dive {
    /// Unique across the document.
    pub uuid: Uuid,
    /// The diver's own numbering; unbounded, duplicates legal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
    /// Local wall clock with its offset when recorded, or a date alone.
    pub started_at: StartedAt,
    /// Seconds in the water, surface time excluded; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submerged_time: Option<u64>,
    /// Free text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// The diver's statement of what kind of dive it was.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub dive_type: Option<DiveType>,
    /// 1–5, 5 the highest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    /// The diver's labels, each 1–64 code points, none twice once trimmed and case-folded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Meters; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<f64>,
    /// Meters; > 0 and ≤ `max_depth`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_depth: Option<f64>,
    /// Degrees Celsius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom_temperature: Option<f64>,
    /// Meters; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<f64>,
    /// Kilograms of ballast; ≥ 0 (`0` is a recorded "no lead").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// The water the dive was in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_type: Option<WaterType>,
    /// Meters above sea level; −450 to 6500.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude: Option<i32>,
    /// Degrees Celsius at the surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub air_temperature: Option<f64>,
    /// The current the diver met.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Current>,
    /// The water's surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waves: Option<Waves>,
    /// The weather at the surface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weather: Option<Weather>,
    /// Where the diver entered the water from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<EntryType>,
    /// The day boat's name, 1–255.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boat_name: Option<String>,
    /// Where the diver entered the water.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_position: Option<Position>,
    /// Where the diver surfaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_position: Option<Position>,
    /// → `trips`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trip_uuid: Option<Uuid>,
    /// → `courses`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_uuid: Option<Uuid>,
    /// → `contacts`: who ran the dive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_uuid: Option<Uuid>,
    /// → `sites`; the first is the primary site.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub site_uuids: Vec<Uuid>,
    /// → `gear`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gear_uuids: Vec<Uuid>,
    /// Species seen, at most one sighting per species.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sightings: Vec<Sighting>,
    /// Who the diver dived with, no person twice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonReference>,
    /// Gas supplies, shared by every recording.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cylinders: Vec<Cylinder>,
    /// One per device that recorded the dive; the first is primary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recordings: Vec<Recording>,
    /// When the record entered the source logbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Dive {
    /// A dive with its required members and nothing else.
    #[must_use]
    pub fn new(uuid: Uuid, started_at: impl Into<StartedAt>) -> Self {
        Self {
            uuid,
            number: None,
            started_at: started_at.into(),
            submerged_time: None,
            notes: None,
            dive_type: None,
            rating: None,
            tags: Vec::new(),
            max_depth: None,
            avg_depth: None,
            bottom_temperature: None,
            visibility: None,
            weight: None,
            water_type: None,
            altitude: None,
            air_temperature: None,
            current: None,
            waves: None,
            weather: None,
            entry_type: None,
            boat_name: None,
            entry_position: None,
            exit_position: None,
            trip_uuid: None,
            course_uuid: None,
            contact_uuid: None,
            site_uuids: Vec::new(),
            gear_uuids: Vec::new(),
            sightings: Vec::new(),
            people: Vec::new(),
            cylinders: Vec::new(),
            recordings: Vec::new(),
            created_at: None,
            extensions: Extensions::new(),
        }
    }
}

/// A dive's `type` (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiveType {
    /// Open-circuit scuba.
    OpenCircuit,
    /// Closed-circuit rebreather.
    ClosedCircuit,
    /// Semi-closed rebreather.
    SemiClosed,
    /// Breath-hold.
    Freedive,
    /// The outing, not the tube.
    Snorkel,
    /// Surface-supplied.
    SurfaceSupplied,
}

/// The water a dive was in, or the water at a site (§6.2, §6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WaterType {
    /// Sea water.
    Salt,
    /// Fresh water.
    Fresh,
    /// Brackish water.
    Brackish,
}

/// The current the diver met (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Current {
    /// No current.
    None,
    /// Light.
    Light,
    /// Moderate.
    Moderate,
    /// Strong.
    Strong,
    /// Extreme.
    Extreme,
}

/// The water's surface (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Waves {
    /// Calm.
    Calm,
    /// Slight.
    Slight,
    /// Moderate.
    Moderate,
    /// Rough.
    Rough,
}

/// The weather at the surface (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Weather {
    /// Clear.
    Clear,
    /// Partly cloudy.
    PartlyCloudy,
    /// Overcast.
    Overcast,
    /// Rain.
    Rain,
    /// Storm.
    Storm,
    /// Snow.
    Snow,
    /// Fog.
    Fog,
}

/// Where a diver enters the water from (§6.2, §6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EntryType {
    /// From the shore.
    Shore,
    /// From a boat.
    Boat,
    /// From a pier, jetty, dock, pontoon or harbour wall.
    Pier,
    /// In a pool.
    Pool,
}

/// One species seen on one dive (§6.3a); at most one of `count` and `abundance`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Sighting {
    /// → `species`.
    pub species_uuid: Uuid,
    /// How many were counted; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// An order-of-magnitude band where there is no number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abundance: Option<Abundance>,
    /// What the diver wrote about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Sighting {
    /// A sighting of a species, seen and not counted.
    #[must_use]
    pub fn new(species_uuid: Uuid) -> Self {
        Self {
            species_uuid,
            count: None,
            abundance: None,
            notes: None,
            extensions: Extensions::new(),
        }
    }
}

/// A sighting's abundance band (§6.3a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Abundance {
    /// 2–10.
    Few,
    /// 11–100.
    Many,
    /// 101 and more.
    Abundant,
}

/// One gas supply on one dive (§6.3). Absent fractions are not air.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Cylinder {
    /// Liters of water capacity; > 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    /// Bar; > 0 and ≤ 350.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_pressure: Option<f64>,
    /// Bar; 0–350 and ≤ `start_pressure`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_pressure: Option<f64>,
    /// Percent; 0–100, and `oxygen + helium` ≤ 100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oxygen: Option<f64>,
    /// Percent; 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub helium: Option<f64>,
    /// Bar; 0.4–2.0, the planned ppO₂ ceiling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppo2_limit: Option<f64>,
    /// The device's label for this gas: the join key from pressure series and gas switches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_number: Option<u32>,
    /// What the gas is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<CylinderRole>,
    /// How it is breathed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CylinderUsage>,
    /// → `gear`: the vessel; no gear item twice in one dive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear_uuid: Option<Uuid>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// A cylinder's `role` (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CylinderRole {
    /// Bottom gas.
    Bottom,
    /// Decompression gas.
    Deco,
    /// Rebreather diluent.
    Diluent,
    /// Rebreather oxygen.
    Oxygen,
}

/// A cylinder's `usage` (§6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CylinderUsage {
    /// Breathed alongside others, e.g. a sidemount pair.
    Parallel,
    /// Carried for a later phase.
    Staged,
}

/// One person on one occasion (§6.20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PersonReference {
    /// → `people`.
    pub person_uuid: Uuid,
    /// What the person was on this occasion; absent means only that they were there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<PersonRole>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl PersonReference {
    /// A reference to a person, optionally in a role.
    #[must_use]
    pub fn new(person_uuid: Uuid, role: Option<PersonRole>) -> Self {
        Self {
            person_uuid,
            role,
            extensions: Extensions::new(),
        }
    }
}

/// A person reference's `role` (§6.20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PersonRole {
    /// Dived alongside the diver.
    Buddy,
    /// Led the dive professionally.
    Guide,
    /// Taught.
    Instructor,
    /// Learned.
    Student,
    /// Came along with no diving role.
    Companion,
}
