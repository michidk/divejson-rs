//! Validation: the schema constraints the types do not carry (lengths, ranges, patterns,
//! conditional members) and every requirement §3 lists as living outside the schema.
//!
//! A parsed [`Document`] already has the schema's shape: types, required members, closed
//! vocabularies, no undefined members, no `null`, uuids and date-times in their grammar.
//! [`Document::validate`] checks the rest and reports every violation it finds, each with a
//! stable [`Code`] and the JSON Pointer of the offending value.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::{
    Certification, Contact, Course, Cylinder, Dive, DiveSite, Document, Event, Extensions,
    ExternalId, Location, PersonReference, Position, Profile, Recording, Species, StoredFile, Uuid,
};

/// What rule a [`Violation`] breaks. The string form ([`Code::as_str`]) is stable: a code is
/// renamed or removed only as a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Code {
    // Schema constraints the types do not carry.
    /// A string is shorter or longer than its bounds, in code points.
    Length,
    /// A number is outside its bounds.
    Range,
    /// A string does not have its required form (sha256, producer key, registry identifier).
    Pattern,
    /// A member required by another member's presence or value is missing.
    MissingMember,
    /// A member is present where another member's presence or value forbids it.
    UnexpectedMember,
    /// A list that holds each value at most once holds one twice.
    DuplicateItem,
    /// A deco model with no members.
    EmptyObject,
    /// A species carries more than one `worms` entry (§6.11).
    MultipleWormsEntries,
    // §3 rule 1.
    /// Two records, stored files or the diver share a uuid (§5.3).
    DuplicateUuid,
    /// A reference resolves to no record of its collection (§5.3).
    DanglingReference,
    // §3 rule 2.
    /// `oxygen + helium > 100` on a cylinder (§6.3).
    OxygenHeliumExceed100,
    /// `end_pressure > start_pressure` on a cylinder (§6.3).
    EndPressureExceedsStart,
    /// `avg_depth > max_depth` on a dive (§6.2).
    AvgDepthExceedsMax,
    /// `depth_from > depth_to` on a dive site (§6.10).
    DepthFromExceedsTo,
    /// `ends_on` before `starts_on` on a trip part or a course (§6.9a, §6.17).
    EndsBeforeStart,
    /// `south > north` on a bounding box (§6.9).
    SouthExceedsNorth,
    // §3 rule 3.
    /// A series' `times` and `values` differ in length (§6.5).
    SeriesLengthMismatch,
    /// A series' `times` is not strictly increasing (§6.5).
    TimesNotIncreasing,
    /// A profile's `duration` is below its latest sample (§6.4).
    DurationTooShort,
    // §3 rule 4.
    /// A recording carries none of device, profile, source files and a readout (§6.4a).
    EmptyRecording,
    // §3 rule 5.
    /// `exported_at` has no UTC offset (§5.2).
    ExportedAtWithoutOffset,
    // §3 rule 6.
    /// `gf_low > gf_high` on a deco model (§6.4c).
    GfLowExceedsHigh,
    // §3 rule 7.
    /// One list of embedded references names one record twice (§6.20, §6.3a, §6.3).
    DuplicateReference,
    /// Two external ids with the same registry and identifier (§6.10, §6.11).
    DuplicateExternalId,
    // §3 rule 8.
    /// Two tags equal once trimmed and case-folded (§6.2, §6.10).
    DuplicateTag,
    /// A site's other name repeats another or the site's name, trimmed and case-folded.
    DuplicateName,
}

impl Code {
    /// The stable machine-readable form, e.g. `"dangling_reference"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::Range => "range",
            Self::Pattern => "pattern",
            Self::MissingMember => "missing_member",
            Self::UnexpectedMember => "unexpected_member",
            Self::DuplicateItem => "duplicate_item",
            Self::EmptyObject => "empty_object",
            Self::MultipleWormsEntries => "multiple_worms_entries",
            Self::DuplicateUuid => "duplicate_uuid",
            Self::DanglingReference => "dangling_reference",
            Self::OxygenHeliumExceed100 => "oxygen_helium_exceed_100",
            Self::EndPressureExceedsStart => "end_pressure_exceeds_start",
            Self::AvgDepthExceedsMax => "avg_depth_exceeds_max",
            Self::DepthFromExceedsTo => "depth_from_exceeds_to",
            Self::EndsBeforeStart => "ends_before_start",
            Self::SouthExceedsNorth => "south_exceeds_north",
            Self::SeriesLengthMismatch => "series_length_mismatch",
            Self::TimesNotIncreasing => "times_not_increasing",
            Self::DurationTooShort => "duration_too_short",
            Self::EmptyRecording => "empty_recording",
            Self::ExportedAtWithoutOffset => "exported_at_without_offset",
            Self::GfLowExceedsHigh => "gf_low_exceeds_high",
            Self::DuplicateReference => "duplicate_reference",
            Self::DuplicateExternalId => "duplicate_external_id",
            Self::DuplicateTag => "duplicate_tag",
            Self::DuplicateName => "duplicate_name",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Violation {
    /// Which rule.
    pub code: Code,
    /// JSON Pointer to the offending value, e.g. `/dives/0/cylinders/1`; `""` is the root.
    pub path: String,
    /// A human-readable sentence; may be reworded in any release.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        write!(f, "{path}: {} [{}]", self.message, self.code)
    }
}

impl Document {
    /// Check every rule the types do not enforce; `Err` lists every violation found.
    ///
    /// # Errors
    /// The violations, in document order.
    pub fn validate(&self) -> Result<(), Vec<Violation>> {
        let mut check = Check::default();
        check.document(self);
        if check.out.is_empty() {
            Ok(())
        } else {
            Err(check.out)
        }
    }
}

/// §3 rule 8's comparison: trimmed of `White_Space`, then Unicode full case folding.
#[must_use]
pub fn folded(text: &str) -> String {
    unicase::UniCase::new(text.trim()).to_folded_case()
}

fn is_producer_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    let lower_digit = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    bytes.next().is_some_and(lower_digit)
        && bytes.all(|b| lower_digit(b) || matches!(b, b'.' | b'_' | b'-'))
}

/// Digits with no leading zero: `^[1-9][0-9]*$`.
fn is_number(text: &str) -> bool {
    !text.starts_with('0') && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn json_pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Every collection's uuids, for resolving references.
#[derive(Default)]
struct Known {
    trips: BTreeSet<Uuid>,
    courses: BTreeSet<Uuid>,
    sites: BTreeSet<Uuid>,
    species: BTreeSet<Uuid>,
    gear: BTreeSet<Uuid>,
    schedules: BTreeSet<Uuid>,
    contacts: BTreeSet<Uuid>,
    people: BTreeSet<Uuid>,
}

#[derive(Default)]
struct Check {
    out: Vec<Violation>,
    uuids: BTreeMap<Uuid, String>,
    known: Known,
}

impl Check {
    fn push(&mut self, code: Code, path: impl Into<String>, message: impl Into<String>) {
        self.out.push(Violation {
            code,
            path: path.into(),
            message: message.into(),
        });
    }

    /// A string's length in code points, `min..=max`.
    fn len(
        &mut self,
        path: impl Into<String>,
        value: Option<&str>,
        min: usize,
        max: Option<usize>,
    ) {
        let Some(value) = value else { return };
        let count = value.chars().count();
        if count < min || max.is_some_and(|max| count > max) {
            let bounds = match max {
                Some(max) => format!("{min}–{max}"),
                None => format!("at least {min}"),
            };
            self.push(
                Code::Length,
                path,
                format!("is {count} characters long; the bounds are {bounds}"),
            );
        }
    }

    fn num(
        &mut self,
        path: impl Into<String>,
        value: Option<f64>,
        ok: fn(f64) -> bool,
        bounds: &str,
    ) {
        if let Some(value) = value.filter(|&v| !ok(v)) {
            self.push(Code::Range, path, format!("{value} is outside {bounds}"));
        }
    }

    fn extensions(&mut self, path: &str, extensions: &Extensions) {
        for key in extensions.keys().filter(|key| !is_producer_key(key)) {
            self.push(
                Code::Pattern,
                format!("{path}/extensions/{}", json_pointer_token(key)),
                format!("{key:?} is not a producer key (spec §5.5)"),
            );
        }
    }

    fn claim(&mut self, uuid: Uuid, path: &str) {
        if let Some(first) = self.uuids.get(&uuid) {
            let message = format!("uuid {uuid} is already used at {first}");
            self.push(Code::DuplicateUuid, path, message);
        } else {
            self.uuids.insert(uuid, path.to_string());
        }
    }

    fn reference(&mut self, path: String, target: Option<Uuid>, collection: &str) {
        let Some(target) = target else { return };
        let resolves = match collection {
            "trips" => self.known.trips.contains(&target),
            "courses" => self.known.courses.contains(&target),
            "sites" => self.known.sites.contains(&target),
            "species" => self.known.species.contains(&target),
            "gear" => self.known.gear.contains(&target),
            "gear_service_schedules" => self.known.schedules.contains(&target),
            "contacts" => self.known.contacts.contains(&target),
            _ => self.known.people.contains(&target),
        };
        if !resolves {
            let message = format!("references {target}, not present in {collection}");
            self.push(Code::DanglingReference, path, message);
        }
    }

    /// A list of bare uuids (`site_uuids`, `gear_uuids`): each resolves, none twice.
    fn reference_list(&mut self, path: &str, targets: &[Uuid], collection: &str) {
        let mut seen = BTreeSet::new();
        for (i, &target) in targets.iter().enumerate() {
            self.reference(format!("{path}/{i}"), Some(target), collection);
            if !seen.insert(target) {
                self.push(
                    Code::DuplicateItem,
                    format!("{path}/{i}"),
                    format!("{target} is listed twice"),
                );
            }
        }
    }

    /// A list of embedded references (§3 rule 7): each resolves, no record twice.
    fn embedded(
        &mut self,
        path: &str,
        targets: impl Iterator<Item = Option<Uuid>>,
        collection: &str,
    ) {
        let mut seen = BTreeMap::new();
        for (i, target) in targets.enumerate() {
            let here = format!("{path}/{i}");
            let Some(target) = target else { continue };
            self.reference(
                format!("{here}/{}", member_of(collection)),
                Some(target),
                collection,
            );
            if let Some(first) = seen.get(&target) {
                let message = format!("{target} is already listed at {path}/{first}");
                self.push(Code::DuplicateReference, here, message);
            } else {
                seen.insert(target, i);
            }
        }
    }

    /// §3 rule 8 over one list, seeded with texts it must not repeat.
    fn folded_unique(
        &mut self,
        path: &str,
        values: &[String],
        mut listed: BTreeMap<String, String>,
        code: Code,
    ) {
        for (i, value) in values.iter().enumerate() {
            let here = format!("{path}/{i}");
            let key = folded(value);
            if let Some(first) = listed.get(&key) {
                let message = format!(
                    "{value:?} is already listed at {first}, compared trimmed and case-folded"
                );
                self.push(code, here, message);
            } else {
                listed.insert(key, here);
            }
        }
    }

    fn tags(&mut self, path: &str, tags: &[String]) {
        for (i, tag) in tags.iter().enumerate() {
            self.len(format!("{path}/{i}"), Some(tag), 1, Some(64));
        }
        self.folded_unique(path, tags, BTreeMap::new(), Code::DuplicateTag);
    }

    fn unique<T: PartialEq + fmt::Debug>(&mut self, path: &str, values: &[T]) {
        for (i, value) in values.iter().enumerate() {
            if values.iter().take(i).any(|earlier| earlier == value) {
                self.push(
                    Code::DuplicateItem,
                    format!("{path}/{i}"),
                    format!("{value:?} is listed twice"),
                );
            }
        }
    }

    fn position(&mut self, path: &str, position: Option<&Position>) {
        let Some(position) = position else { return };
        self.num(
            format!("{path}/latitude"),
            Some(position.latitude),
            |v| (-90.0..=90.0).contains(&v),
            "−90–90",
        );
        self.num(
            format!("{path}/longitude"),
            Some(position.longitude),
            |v| (-180.0..=180.0).contains(&v),
            "−180–180",
        );
        self.extensions(path, &position.extensions);
    }

    fn location(&mut self, path: &str, location: Option<&Location>) {
        let Some(location) = location else { return };
        self.len(format!("{path}/name"), Some(&location.name), 1, Some(255));
        self.position(&format!("{path}/position"), location.position.as_ref());
        if let Some(bbox) = &location.bbox {
            let here = format!("{path}/bbox");
            if location.position.is_none() {
                self.push(
                    Code::MissingMember,
                    path,
                    "a location with a bbox needs a position (spec §6.9)",
                );
            }
            let lat = |v: f64| (-90.0..=90.0).contains(&v);
            let lon = |v: f64| (-180.0..=180.0).contains(&v);
            self.num(format!("{here}/south"), Some(bbox.south), lat, "−90–90");
            self.num(format!("{here}/north"), Some(bbox.north), lat, "−90–90");
            self.num(format!("{here}/west"), Some(bbox.west), lon, "−180–180");
            self.num(format!("{here}/east"), Some(bbox.east), lon, "−180–180");
            if bbox.south > bbox.north {
                self.push(
                    Code::SouthExceedsNorth,
                    &here,
                    "south exceeds north (spec §6.9)",
                );
            }
            self.extensions(&here, &bbox.extensions);
        }
        self.extensions(path, &location.extensions);
    }

    fn stored_file(&mut self, path: &str, file: Option<&StoredFile>) {
        let Some(file) = file else { return };
        self.claim(file.uuid, path);
        self.len(
            format!("{path}/original_filename"),
            Some(&file.original_filename),
            1,
            Some(255),
        );
        self.len(
            format!("{path}/content_type"),
            Some(&file.content_type),
            1,
            Some(64),
        );
        self.len(
            format!("{path}/archive_path"),
            file.archive_path.as_deref(),
            1,
            None,
        );
        let hex = file.sha256.len() == 64
            && file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            self.push(
                Code::Pattern,
                format!("{path}/sha256"),
                "is not 64 lowercase hex digits",
            );
        }
        self.extensions(path, &file.extensions);
    }

    fn external_ids(&mut self, path: &str, ids: &[ExternalId]) {
        let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for (i, id) in ids.iter().enumerate() {
            let here = format!("{path}/{i}");
            if !is_producer_key(&id.registry) {
                self.push(
                    Code::Pattern,
                    format!("{here}/registry"),
                    format!("{:?} is not a producer key", id.registry),
                );
            }
            self.len(
                format!("{here}/identifier"),
                Some(&id.identifier),
                1,
                Some(255),
            );
            let text = id.identifier.as_str();
            let form = match id.registry.as_str() {
                "wikidata" => Some((
                    text.strip_prefix('Q').is_some_and(is_number),
                    "Q and a number",
                )),
                "openstreetmap" => Some((
                    text.split_once('/').is_some_and(|(kind, number)| {
                        matches!(kind, "node" | "way" | "relation") && is_number(number)
                    }),
                    "node/, way/ or relation/ and a number",
                )),
                "worms" => Some((is_number(text), "an AphiaID's digits")),
                _ => None,
            };
            if let Some((false, expected)) = form {
                let message = format!("{text:?} is not a {} identifier: {expected}", id.registry);
                self.push(Code::Pattern, format!("{here}/identifier"), message);
            }
            if let Some(first) = seen.get(&(id.registry.as_str(), text)) {
                let message = format!(
                    "the {} entry {text:?} is already listed at {path}/{first}",
                    id.registry
                );
                self.push(Code::DuplicateExternalId, &here, message);
            } else {
                seen.insert((&id.registry, text), i);
            }
            self.extensions(&here, &id.extensions);
        }
    }

    fn people(&mut self, path: &str, people: &[PersonReference]) {
        self.embedded(path, people.iter().map(|r| Some(r.person_uuid)), "people");
        for (i, reference) in people.iter().enumerate() {
            self.extensions(&format!("{path}/{i}"), &reference.extensions);
        }
    }

    fn agency_other(&mut self, path: &str, is_other: bool, agency_other: Option<&str>) {
        match (is_other, agency_other) {
            (true, None) => self.push(
                Code::MissingMember,
                path,
                "agency \"other\" needs agency_other (spec §6.16)",
            ),
            (false, Some(_)) => self.push(
                Code::UnexpectedMember,
                format!("{path}/agency_other"),
                "agency_other is only written beside agency \"other\" (spec §6.16)",
            ),
            _ => {}
        }
        self.len(format!("{path}/agency_other"), agency_other, 1, Some(64));
    }

    fn dates(&mut self, path: &str, starts_on: Option<crate::Date>, ends_on: Option<crate::Date>) {
        if let (Some(start), Some(end)) = (starts_on, ends_on) {
            if end < start {
                self.push(
                    Code::EndsBeforeStart,
                    path,
                    format!("ends_on {end} precedes starts_on {start}"),
                );
            }
        }
    }

    fn document(&mut self, doc: &Document) {
        if doc.exported_at.offset().is_none() {
            self.push(
                Code::ExportedAtWithoutOffset,
                "/exported_at",
                "must carry a UTC offset — it is generated, not recorded history (spec §5.2)",
            );
        }
        if let Some(generator) = &doc.generator {
            self.len("/generator/name", Some(&generator.name), 1, None);
            self.extensions("/generator", &generator.extensions);
        }
        if let Some(diver) = &doc.diver {
            self.diver(diver);
        }
        self.extensions("", &doc.extensions);

        self.claim_all(doc);

        for (i, dive) in doc.dives.iter().enumerate() {
            self.dive(&format!("/dives/{i}"), dive);
        }
        for (i, trip) in doc.trips.iter().enumerate() {
            let here = format!("/trips/{i}");
            self.len(format!("{here}/name"), Some(&trip.name), 1, Some(255));
            for (j, part) in trip.parts.iter().enumerate() {
                let part_path = format!("{here}/parts/{j}");
                self.dates(&part_path, part.starts_on, part.ends_on);
                self.location(&format!("{part_path}/location"), part.location.as_ref());
                self.reference(
                    format!("{part_path}/accommodation_uuid"),
                    part.accommodation_uuid,
                    "contacts",
                );
                self.extensions(&part_path, &part.extensions);
            }
            self.people(&format!("{here}/people"), &trip.people);
            self.extensions(&here, &trip.extensions);
        }
        for (i, course) in doc.courses.iter().enumerate() {
            self.course(&format!("/courses/{i}"), course);
        }
        for (i, site) in doc.sites.iter().enumerate() {
            self.site(&format!("/sites/{i}"), site);
        }
        for (i, species) in doc.species.iter().enumerate() {
            self.species(&format!("/species/{i}"), species);
        }
        self.gear(doc);
        for (i, certification) in doc.certifications.iter().enumerate() {
            self.certification(&format!("/certifications/{i}"), certification);
        }
        for (i, contact) in doc.contacts.iter().enumerate() {
            self.contact(&format!("/contacts/{i}"), contact);
        }
        for (i, person) in doc.people.iter().enumerate() {
            let here = format!("/people/{i}");
            self.len(format!("{here}/name"), Some(&person.name), 1, Some(255));
            self.len(
                format!("{here}/email"),
                person.email.as_deref(),
                0,
                Some(255),
            );
            self.len(
                format!("{here}/phone"),
                person.phone.as_deref(),
                0,
                Some(32),
            );
            self.extensions(&here, &person.extensions);
        }
    }

    /// Gear items, sets, service schedules and service records (§6.12–6.15).
    fn gear(&mut self, doc: &Document) {
        for (i, item) in doc.gear.iter().enumerate() {
            let here = format!("/gear/{i}");
            self.len(format!("{here}/name"), Some(&item.name), 1, Some(255));
            self.len(format!("{here}/brand"), item.brand.as_deref(), 0, Some(255));
            self.len(
                format!("{here}/serial"),
                item.serial.as_deref(),
                1,
                Some(64),
            );
            self.num(format!("{here}/volume"), item.volume, |v| v > 0.0, "> 0");
            self.num(
                format!("{here}/working_pressure"),
                item.working_pressure,
                |v| v > 0.0 && v <= 350.0,
                "> 0 and ≤ 350",
            );
            self.extensions(&here, &item.extensions);
        }
        for (i, set) in doc.gear_sets.iter().enumerate() {
            let here = format!("/gear_sets/{i}");
            self.len(format!("{here}/name"), Some(&set.name), 1, Some(255));
            self.num(format!("{here}/weight"), set.weight, |v| v >= 0.0, "≥ 0");
            self.reference_list(&format!("{here}/gear_uuids"), &set.gear_uuids, "gear");
            self.extensions(&here, &set.extensions);
        }
        for (i, schedule) in doc.gear_service_schedules.iter().enumerate() {
            let here = format!("/gear_service_schedules/{i}");
            self.reference(
                format!("{here}/gear_uuid"),
                Some(schedule.gear_uuid),
                "gear",
            );
            self.len(
                format!("{here}/label"),
                schedule.label.as_deref(),
                0,
                Some(120),
            );
            let intervals = [
                ("interval_months", schedule.interval_months),
                ("interval_dives", schedule.interval_dives),
                ("interval_hours", schedule.interval_hours),
            ];
            if intervals.iter().all(|(_, value)| value.is_none()) {
                self.push(
                    Code::MissingMember,
                    &here,
                    "a schedule needs at least one interval (spec §6.14)",
                );
            }
            for (member, value) in intervals {
                if value == Some(0) {
                    self.push(Code::Range, format!("{here}/{member}"), "0 is outside > 0");
                }
            }
            self.extensions(&here, &schedule.extensions);
        }
        for (i, record) in doc.gear_service_records.iter().enumerate() {
            let here = format!("/gear_service_records/{i}");
            self.reference(format!("{here}/gear_uuid"), Some(record.gear_uuid), "gear");
            self.reference(
                format!("{here}/gear_service_schedule_uuid"),
                record.gear_service_schedule_uuid,
                "gear_service_schedules",
            );
            self.reference(
                format!("{here}/contact_uuid"),
                record.contact_uuid,
                "contacts",
            );
            self.len(
                format!("{here}/label"),
                record.label.as_deref(),
                0,
                Some(120),
            );
            self.len(
                format!("{here}/performed_by"),
                record.performed_by.as_deref(),
                0,
                Some(255),
            );
            self.extensions(&here, &record.extensions);
        }
    }

    fn diver(&mut self, diver: &crate::Diver) {
        if let Some(uuid) = diver.uuid {
            self.claim(uuid, "/diver");
        }
        self.len("/diver/name", diver.name.as_deref(), 0, Some(255));
        self.len("/diver/email", diver.email.as_deref(), 0, Some(255));
        self.len("/diver/phone", diver.phone.as_deref(), 0, Some(32));
        for (i, contact) in diver.emergency_contacts.iter().enumerate() {
            let here = format!("/diver/emergency_contacts/{i}");
            self.len(format!("{here}/name"), Some(&contact.name), 1, Some(255));
            self.len(
                format!("{here}/phone"),
                contact.phone.as_deref(),
                0,
                Some(32),
            );
            self.len(
                format!("{here}/relationship"),
                contact.relationship.as_deref(),
                0,
                Some(64),
            );
            self.extensions(&here, &contact.extensions);
        }
        for (i, insurance) in diver.insurances.iter().enumerate() {
            let here = format!("/diver/insurances/{i}");
            self.len(
                format!("{here}/provider"),
                Some(&insurance.provider),
                1,
                Some(255),
            );
            self.len(
                format!("{here}/number"),
                insurance.number.as_deref(),
                0,
                Some(64),
            );
            self.extensions(&here, &insurance.extensions);
        }
        self.stored_file("/diver/portrait_file", diver.portrait_file.as_ref());
        self.extensions("/diver", &diver.extensions);
    }

    /// Every top-level record's uuid into the one identifier space, and each collection's
    /// set for resolving references.
    fn claim_all(&mut self, doc: &Document) {
        fn each<T>(
            check: &mut Check,
            name: &str,
            rows: &[T],
            uuid: fn(&T) -> Uuid,
        ) -> BTreeSet<Uuid> {
            for (i, row) in rows.iter().enumerate() {
                check.claim(uuid(row), &format!("/{name}/{i}"));
            }
            rows.iter().map(uuid).collect()
        }
        each(self, "dives", &doc.dives, |r| r.uuid);
        self.known.trips = each(self, "trips", &doc.trips, |r| r.uuid);
        self.known.courses = each(self, "courses", &doc.courses, |r| r.uuid);
        self.known.sites = each(self, "sites", &doc.sites, |r| r.uuid);
        self.known.species = each(self, "species", &doc.species, |r| r.uuid);
        self.known.gear = each(self, "gear", &doc.gear, |r| r.uuid);
        each(self, "gear_sets", &doc.gear_sets, |r| r.uuid);
        self.known.schedules = each(
            self,
            "gear_service_schedules",
            &doc.gear_service_schedules,
            |r| r.uuid,
        );
        each(
            self,
            "gear_service_records",
            &doc.gear_service_records,
            |r| r.uuid,
        );
        each(self, "certifications", &doc.certifications, |r| r.uuid);
        self.known.contacts = each(self, "contacts", &doc.contacts, |r| r.uuid);
        self.known.people = each(self, "people", &doc.people, |r| r.uuid);
    }

    fn dive(&mut self, here: &str, dive: &Dive) {
        let p = |member: &str| format!("{here}/{member}");
        if dive.submerged_time == Some(0) {
            self.push(Code::Range, p("submerged_time"), "0 is outside > 0");
        }
        if let Some(rating) = dive.rating.filter(|r| !(1..=5).contains(r)) {
            self.push(Code::Range, p("rating"), format!("{rating} is outside 1–5"));
        }
        self.tags(&p("tags"), &dive.tags);
        self.num(p("max_depth"), dive.max_depth, |v| v > 0.0, "> 0");
        self.num(p("avg_depth"), dive.avg_depth, |v| v > 0.0, "> 0");
        self.num(p("visibility"), dive.visibility, |v| v >= 0.0, "≥ 0");
        self.num(p("weight"), dive.weight, |v| v >= 0.0, "≥ 0");
        self.num(
            p("altitude"),
            dive.altitude.map(f64::from),
            |v| (-450.0..=6500.0).contains(&v),
            "−450–6500",
        );
        if let (Some(avg), Some(max)) = (dive.avg_depth, dive.max_depth) {
            if avg > max {
                self.push(
                    Code::AvgDepthExceedsMax,
                    here,
                    format!("avg_depth {avg} exceeds max_depth {max}"),
                );
            }
        }
        self.len(p("boat_name"), dive.boat_name.as_deref(), 1, Some(255));
        self.position(&p("entry_position"), dive.entry_position.as_ref());
        self.position(&p("exit_position"), dive.exit_position.as_ref());
        self.reference(p("trip_uuid"), dive.trip_uuid, "trips");
        self.reference(p("course_uuid"), dive.course_uuid, "courses");
        self.reference(p("contact_uuid"), dive.contact_uuid, "contacts");
        self.reference_list(&p("site_uuids"), &dive.site_uuids, "sites");
        self.reference_list(&p("gear_uuids"), &dive.gear_uuids, "gear");

        self.embedded(
            &p("sightings"),
            dive.sightings.iter().map(|s| Some(s.species_uuid)),
            "species",
        );
        for (i, sighting) in dive.sightings.iter().enumerate() {
            let at = format!("{here}/sightings/{i}");
            if sighting.count == Some(0) {
                self.push(Code::Range, format!("{at}/count"), "0 is outside > 0");
            }
            if sighting.count.is_some() && sighting.abundance.is_some() {
                self.push(
                    Code::UnexpectedMember,
                    &at,
                    "a sighting carries at most one of count and abundance (spec §6.3a)",
                );
            }
            self.extensions(&at, &sighting.extensions);
        }
        self.people(&p("people"), &dive.people);
        self.embedded(
            &p("cylinders"),
            dive.cylinders.iter().map(|c| c.gear_uuid),
            "gear",
        );
        for (i, cylinder) in dive.cylinders.iter().enumerate() {
            self.cylinder(&format!("{here}/cylinders/{i}"), cylinder);
        }
        for (i, recording) in dive.recordings.iter().enumerate() {
            self.recording(&format!("{here}/recordings/{i}"), recording);
        }
        self.extensions(here, &dive.extensions);
    }

    fn cylinder(&mut self, here: &str, cylinder: &Cylinder) {
        let p = |member: &str| format!("{here}/{member}");
        let percent = |v: f64| (0.0..=100.0).contains(&v);
        self.num(p("volume"), cylinder.volume, |v| v > 0.0, "> 0");
        self.num(
            p("start_pressure"),
            cylinder.start_pressure,
            |v| v > 0.0 && v <= 350.0,
            "> 0 and ≤ 350",
        );
        self.num(
            p("end_pressure"),
            cylinder.end_pressure,
            |v| (0.0..=350.0).contains(&v),
            "0–350",
        );
        self.num(p("oxygen"), cylinder.oxygen, percent, "0–100");
        self.num(p("helium"), cylinder.helium, percent, "0–100");
        self.num(
            p("ppo2_limit"),
            cylinder.ppo2_limit,
            |v| (0.4..=2.0).contains(&v),
            "0.4–2.0",
        );
        if let (Some(oxygen), Some(helium)) = (cylinder.oxygen, cylinder.helium) {
            if oxygen + helium > 100.0 {
                self.push(
                    Code::OxygenHeliumExceed100,
                    here,
                    "oxygen + helium exceeds 100 percent",
                );
            }
        }
        if let (Some(start), Some(end)) = (cylinder.start_pressure, cylinder.end_pressure) {
            if end > start {
                self.push(
                    Code::EndPressureExceedsStart,
                    here,
                    "end_pressure exceeds start_pressure",
                );
            }
        }
        self.extensions(here, &cylinder.extensions);
    }

    fn recording(&mut self, here: &str, recording: &Recording) {
        let p = |member: &str| format!("{here}/{member}");
        let readouts = [
            ("surface_pressure", recording.surface_pressure),
            ("cns_start", recording.cns_start),
            ("cns_end", recording.cns_end),
            ("otu_start", recording.otu_start),
            ("otu_end", recording.otu_end),
        ];
        self.num(
            p("surface_pressure"),
            recording.surface_pressure,
            |v| (0.4..=1.2).contains(&v),
            "0.4–1.2",
        );
        for (member, value) in readouts.iter().skip(1) {
            self.num(p(member), *value, |v| v >= 0.0, "≥ 0");
        }
        let has_content = recording.device.is_some()
            || recording.profile.is_some()
            || !recording.source_files.is_empty()
            || readouts.iter().any(|(_, value)| value.is_some());
        if !has_content {
            self.push(
                Code::EmptyRecording,
                here,
                "a recording carries at least one of device, profile, source_files and a readout (spec §6.4a)",
            );
        }
        for (i, file) in recording.source_files.iter().enumerate() {
            self.stored_file(&format!("{here}/source_files/{i}"), Some(file));
        }
        if let Some(device) = &recording.device {
            let at = p("device");
            for (member, value, max) in [
                ("brand", &device.brand, 64),
                ("model", &device.model, 64),
                ("serial", &device.serial, 64),
                ("firmware", &device.firmware, 32),
                ("name", &device.name, 64),
            ] {
                self.len(format!("{at}/{member}"), value.as_deref(), 1, Some(max));
            }
            self.extensions(&at, &device.extensions);
        }
        if let Some(model) = &recording.deco_model {
            let at = p("deco_model");
            if model == &crate::DecoModel::default() {
                self.push(
                    Code::EmptyObject,
                    &at,
                    "a deco model with no members is not written (spec §6.4c)",
                );
            }
            self.len(format!("{at}/name"), model.name.as_deref(), 1, Some(64));
            for (member, value) in [("gf_low", model.gf_low), ("gf_high", model.gf_high)] {
                if let Some(value) = value.filter(|v| *v > 100) {
                    self.push(
                        Code::Range,
                        format!("{at}/{member}"),
                        format!("{value} is outside 0–100"),
                    );
                }
            }
            match (model.gf_low, model.gf_high) {
                (Some(low), Some(high)) if low > high => {
                    self.push(
                        Code::GfLowExceedsHigh,
                        &at,
                        "gf_low exceeds gf_high (spec §3, §6.4c)",
                    );
                }
                (Some(_), None) | (None, Some(_)) => {
                    self.push(
                        Code::MissingMember,
                        &at,
                        "gf_low and gf_high are written both or neither (spec §6.4c)",
                    );
                }
                _ => {}
            }
            self.extensions(&at, &model.extensions);
        }
        if let Some(profile) = &recording.profile {
            self.profile(&p("profile"), profile);
        }
        self.extensions(here, &recording.extensions);
    }

    fn profile(&mut self, here: &str, profile: &Profile) {
        let mut latest = 0;
        for (name, series) in profile.channels() {
            let at = format!("{here}/{name}");
            latest = latest.max(self.series(&at, &series.times, &series.values));
            let floor = match name {
                "ndl" | "tts" | "ppo2" | "cns" | "gradient_factor" | "surface_gradient_factor" => {
                    Some(0)
                }
                _ => None,
            };
            if let Some(floor) = floor {
                for (i, value) in series
                    .values
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| **v < floor)
                {
                    self.push(
                        Code::Range,
                        format!("{at}/values/{i}"),
                        format!("{value} is below {floor}"),
                    );
                }
            }
            self.extensions(&at, &series.extensions);
        }
        for (i, series) in profile.pressures.iter().enumerate() {
            let at = format!("{here}/pressures/{i}");
            latest = latest.max(self.series(&at, &series.times, &series.values));
            self.extensions(&at, &series.extensions);
        }
        // Events may fall after the last sample (spec §6.4), so they do not count here.
        if profile.duration < latest {
            let message = format!(
                "duration {} does not cover the latest sample at {latest} (spec §6.4)",
                profile.duration
            );
            self.push(Code::DurationTooShort, format!("{here}/duration"), message);
        }
        for (i, event) in profile.events.iter().enumerate() {
            self.event(&format!("{here}/events/{i}"), event);
        }
        self.extensions(here, &profile.extensions);
    }

    /// One channel's integrity; returns its latest time.
    fn series(&mut self, here: &str, times: &[u64], values: &[i64]) -> u64 {
        if times.len() != values.len() {
            let message = format!(
                "times has {} samples but values has {}",
                times.len(),
                values.len()
            );
            self.push(Code::SeriesLengthMismatch, here, message);
        }
        if times
            .windows(2)
            .any(|pair| matches!(pair, [earlier, later] if later <= earlier))
        {
            self.push(
                Code::TimesNotIncreasing,
                format!("{here}/times"),
                "times is not strictly increasing",
            );
        }
        times.iter().copied().max().unwrap_or(0)
    }

    fn event(&mut self, here: &str, event: &Event) {
        if event.event_type.is_none() && event.label.as_deref().is_none_or(str::is_empty) {
            self.push(
                Code::MissingMember,
                here,
                "an event without a type needs a non-empty label (spec §6.6)",
            );
        }
        self.extensions(here, &event.extensions);
    }

    fn course(&mut self, here: &str, course: &Course) {
        self.len(format!("{here}/name"), Some(&course.name), 1, Some(255));
        self.agency_other(
            here,
            course.agency == Some(crate::Agency::Other),
            course.agency_other.as_deref(),
        );
        self.dates(here, course.starts_on, course.ends_on);
        self.len(
            format!("{here}/instructor_number"),
            course.instructor_number.as_deref(),
            0,
            Some(64),
        );
        self.reference(
            format!("{here}/contact_uuid"),
            course.contact_uuid,
            "contacts",
        );
        self.people(&format!("{here}/people"), &course.people);
        self.extensions(here, &course.extensions);
    }

    fn site(&mut self, here: &str, site: &DiveSite) {
        let p = |member: &str| format!("{here}/{member}");
        self.len(p("name"), Some(&site.name), 1, Some(255));
        for (i, name) in site.other_names.iter().enumerate() {
            self.len(format!("{here}/other_names/{i}"), Some(name), 1, Some(255));
        }
        let mut listed = BTreeMap::new();
        listed.insert(folded(&site.name), p("name"));
        self.folded_unique(
            &p("other_names"),
            &site.other_names,
            listed,
            Code::DuplicateName,
        );
        self.location(&p("location"), site.location.as_ref());
        self.position(&p("position"), site.position.as_ref());
        self.external_ids(&p("external_ids"), &site.external_ids);
        self.num(p("depth_from"), site.depth_from, |v| v >= 0.0, "≥ 0");
        self.num(p("depth_to"), site.depth_to, |v| v >= 0.0, "≥ 0");
        if let (Some(from), Some(to)) = (site.depth_from, site.depth_to) {
            if from > to {
                self.push(
                    Code::DepthFromExceedsTo,
                    here,
                    format!("depth_from {from} exceeds depth_to {to}"),
                );
            }
        }
        self.num(
            p("altitude"),
            site.altitude.map(f64::from),
            |v| (-450.0..=6500.0).contains(&v),
            "−450–6500",
        );
        self.unique(&p("entry_types"), &site.entry_types);
        self.tags(&p("tags"), &site.tags);
        self.extensions(here, &site.extensions);
    }

    fn species(&mut self, here: &str, species: &Species) {
        self.external_ids(&format!("{here}/external_ids"), &species.external_ids);
        if species
            .external_ids
            .iter()
            .filter(|id| id.registry == "worms")
            .count()
            > 1
        {
            self.push(
                Code::MultipleWormsEntries,
                format!("{here}/external_ids"),
                "a species carries at most one worms entry (spec §6.11)",
            );
        }
        self.len(
            format!("{here}/scientific_name"),
            species.scientific_name.as_deref(),
            1,
            Some(255),
        );
        self.len(
            format!("{here}/common_name"),
            species.common_name.as_deref(),
            1,
            Some(255),
        );
        self.len(format!("{here}/rank"), species.rank.as_deref(), 0, Some(64));
        if species.scientific_name.is_none()
            && species.common_name.is_none()
            && species.external_ids.is_empty()
        {
            self.push(
                Code::MissingMember,
                here,
                "a species needs a scientific_name, a common_name or an external id (spec §6.11)",
            );
        }
        self.extensions(here, &species.extensions);
    }

    fn certification(&mut self, here: &str, certification: &Certification) {
        let p = |member: &str| format!("{here}/{member}");
        self.agency_other(
            here,
            certification.agency == crate::Agency::Other,
            certification.agency_other.as_deref(),
        );
        self.len(p("name"), Some(&certification.name), 1, Some(255));
        self.len(p("number"), certification.number.as_deref(), 0, Some(64));
        self.len(
            p("instructor_number"),
            certification.instructor_number.as_deref(),
            0,
            Some(64),
        );
        self.reference(
            p("instructor_uuid"),
            certification.instructor_uuid,
            "people",
        );
        self.reference(p("contact_uuid"), certification.contact_uuid, "contacts");
        self.reference(p("course_uuid"), certification.course_uuid, "courses");
        self.stored_file(&p("front_file"), certification.front_file.as_ref());
        self.stored_file(&p("back_file"), certification.back_file.as_ref());
        self.extensions(here, &certification.extensions);
    }

    fn contact(&mut self, here: &str, contact: &Contact) {
        let p = |member: &str| format!("{here}/{member}");
        self.len(p("name"), Some(&contact.name), 1, Some(255));
        self.unique(&p("roles"), &contact.roles);
        self.len(p("phone"), contact.phone.as_deref(), 0, Some(32));
        self.len(p("email"), contact.email.as_deref(), 0, Some(255));
        self.len(p("website"), contact.website.as_deref(), 0, Some(512));
        if let Some(address) = &contact.address {
            let at = p("address");
            for (member, value, max) in [
                ("street", &address.street, 255),
                ("city", &address.city, 255),
                ("postcode", &address.postcode, 32),
                ("region", &address.region, 255),
            ] {
                self.len(format!("{at}/{member}"), value.as_deref(), 0, Some(max));
            }
            self.len(
                format!("{at}/country"),
                Some(&address.country),
                1,
                Some(255),
            );
            self.extensions(&at, &address.extensions);
        }
        self.position(&p("position"), contact.position.as_ref());
        self.extensions(here, &contact.extensions);
    }
}

/// The member of an embedded reference that names its record.
fn member_of(collection: &str) -> &'static str {
    match collection {
        "species" => "species_uuid",
        "gear" => "gear_uuid",
        _ => "person_uuid",
    }
}
