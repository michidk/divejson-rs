//! UDDF into DiveJSON, following `docs/uddf-mapping.md`.

use std::collections::{HashMap, HashSet};

use ::uddf as u;
use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{
    ID_NAMESPACE, KELVIN_OFFSET, LITRES_PER_CUBIC_METRE, Note, NoteKind, PASCAL_PER_BAR, Report,
    in_seconds, milliseconds, rounded, tidy,
};
use crate::{
    Address, Contact, ContactRole, Current, Cylinder, Date, DateTime, DecoAlgorithm, DecoModel,
    Device, Dive, DiveSite, DiveType, Diver, Document, EntryType, Error, Event, EventType,
    GearItem, GearType, Generator, Insurance, Location, Material, Person, PersonReference,
    PersonRole, Position, PressureSeries, Profile, Recording, RecordingMode, Series, StartedAt,
    Trip, TripPart, Uuid, folded,
};

/// §6's cap on a REQUIRED name, and every other name this reader fills.
const MAX_NAME: usize = 255;
const MAX_SERIAL: usize = 64;
const MAX_PHONE: usize = 32;
const MAX_EMAIL: usize = 255;
const MAX_WEBSITE: usize = 512;
const MAX_POSTCODE: usize = 32;
/// §6.4b's cap on each device string.
const MAX_DEVICE: usize = 64;

/// The generators whose `Z` is the local wall clock and whose gradient factors are whole
/// percent: the exact `<generator><name>`, with the `<manufacturer>` id beside it.
const SHEARWATER: (&str, &str) = ("Shearwater Cloud Desktop", "Shearwater_Research_Inc");

const LOCAL_CLOCK_NOTE: &str = "The application that wrote this file marks the local time with a 'Z', the sign for UTC; read as a local time without a time zone.";
const PERCENT_GRADIENT_NOTE: &str = "The application that wrote this file gives gradient factors in whole percent where the UDDF documentation's examples show a fraction; read as percent.";

/// Reads a UDDF document as DiveJSON, with the report of what it could not carry.
///
/// `exported_at` becomes the document's `exported_at`: the moment of conversion, passed in
/// so that the output is a function of the input. The reader is lenient (§ *Schema validity
/// is never a precondition*): a value that cannot be read is absent and reported, and every
/// way the source can be wrong resolves to an omission and a note.
///
/// # Errors
///
/// [`Error::Uddf`] for malformed XML, a `<!DOCTYPE>` or a root other than `<uddf>`;
/// [`Error::Invalid`] if the document produced breaks a rule, which is a bug in this reader.
pub fn read(text: &str, exported_at: DateTime) -> Result<(Document, Report), Error> {
    let (source, invalid) = u::from_str_lenient(text)?;
    let mut reader = Reader::new(&source);
    for value in invalid {
        reader.note(
            "$",
            format!(
                "{} is {:?}, not {}; it is read as not recorded.",
                value.path, value.value, value.expected
            ),
            NoteKind::Dropped,
            "value_unreadable",
            None,
        );
    }
    let doc = reader.run(exported_at);
    doc.validate().map_err(Error::Invalid)?;
    Ok((doc, reader.notes))
}

/// A cylinder's gas, read off a `<mix>`.
#[derive(Clone, Copy, Default)]
struct Gas {
    oxygen: Option<f64>,
    helium: Option<f64>,
    ppo2_limit: Option<f64>,
}

/// A contact's members beyond its name and roles.
#[derive(Default)]
struct Shape {
    phone: Option<String>,
    email: Option<String>,
    website: Option<String>,
    address: Option<Address>,
    notes: Option<String>,
}

/// What `<informationafterdive>`'s neighbours of a contact's shape carry.
struct ShapeOf<'a> {
    tag: &'a str,
    id: Option<&'a str>,
    name: Option<&'a str>,
    address: Option<&'a u::Address>,
    contact: Option<&'a u::Contact>,
    notes: &'a [u::NoteItem],
    /// What else the element records that §6.18 has no member for.
    residue: Vec<String>,
}

struct Reader<'a> {
    source: &'a u::Uddf,
    refs: u::References<'a>,
    notes: Report,
    claimed: HashSet<uuid::Uuid>,
    site_uuids: HashMap<&'a str, Uuid>,
    trip_uuids: HashMap<&'a str, Uuid>,
    gear_uuids: HashMap<&'a str, Uuid>,
    tank_volumes: HashMap<&'a str, f64>,
    computers: HashMap<&'a str, &'a u::EquipmentPiece>,
    mixes: HashMap<&'a str, Gas>,
    deco_models: HashMap<&'a str, &'a u::DecoModel>,
    contact_uuids: HashMap<&'a str, Uuid>,
    unread_contacts: HashSet<&'a str>,
    contacts: Vec<Contact>,
    contact_names: HashMap<String, usize>,
    person_uuids: HashMap<&'a str, Uuid>,
    unread_people: HashSet<&'a str>,
    students: HashSet<&'a str>,
    guides: HashMap<&'a str, Option<&'a str>>,
    linked_guides: HashSet<&'a str>,
    local_clock_with_z: bool,
    percent_gradient_factors: bool,
    reported_gradient_scale: bool,
}

fn text(value: Option<&String>) -> Option<&str> {
    value
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
}

fn notes_text(notes: &[u::NoteItem]) -> Option<String> {
    u::notes_text(notes).filter(|text| !text.trim().is_empty())
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && value.abs() <= f64::MAX / 1000.0)
}

fn told(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn is_email(value: &str) -> bool {
    value.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && !domain.is_empty()
            && !domain.contains('@')
            && !value.chars().any(char::is_whitespace)
    })
}

fn is_absolute_uri(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && !rest.is_empty()
        && !rest.chars().any(char::is_whitespace)
}

fn is_uuid_text(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The UUID a source id already carries: a canonical UUID, alone or behind a short
/// alphabetic prefix (`dive-0198…`).
fn embedded_uuid(source_id: &str) -> Option<uuid::Uuid> {
    let candidate = match source_id.split_once('-') {
        Some((head, tail))
            if !head.is_empty()
                && head.chars().count() <= 12
                && head.chars().all(char::is_alphabetic)
                && is_uuid_text(tail) =>
        {
            tail
        }
        _ if is_uuid_text(source_id) => source_id,
        _ => return None,
    };
    uuid::Uuid::parse_str(candidate).ok()
}

/// A `<tankvolume>` as litres, and whether it had to be read as litres already: UDDF's
/// cubic metres below 1, litres at or above (`docs/uddf-mapping.md`, *Ambiguities*).
fn volume_litres(raw: f64) -> (f64, bool) {
    if raw < 1.0 {
        (tidy(raw * LITRES_PER_CUBIC_METRE), false)
    } else {
        (raw, true)
    }
}

/// An `<o2>`/`<he>` as percent, and whether it was percent already (above 1).
fn gas_percent(raw: f64) -> (f64, bool) {
    if raw <= 1.0 {
        (tidy(raw * 100.0), false)
    } else {
        (raw, true)
    }
}

fn date_of(value: Option<&u::DateTime>) -> Option<Date> {
    value.and_then(|value| Date::try_from(value.date).ok())
}

fn date_text(date: jiff::civil::Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// A contact's role in §6.18's own order.
fn role_rank(role: ContactRole) -> u8 {
    match role {
        ContactRole::DiveCenter => 0,
        ContactRole::School => 1,
        ContactRole::Shop => 2,
        ContactRole::Accommodation => 3,
        ContactRole::Liveaboard => 4,
        ContactRole::Club => 5,
        _ => 6,
    }
}

fn gear_type(kind: u::EquipmentKind) -> Option<GearType> {
    use u::EquipmentKind as K;
    Some(match kind {
        K::Boots => GearType::Boots,
        K::BuoyancyControlDevice => GearType::Bcd,
        K::Camera | K::VideoCamera => GearType::Camera,
        K::Compass => GearType::Compass,
        K::DiveComputer => GearType::Computer,
        K::Fins => GearType::Fins,
        K::Gloves => GearType::Gloves,
        K::Knife => GearType::Knife,
        K::Light => GearType::Light,
        K::Mask => GearType::Mask,
        K::Rebreather => GearType::Rebreather,
        K::Regulator => GearType::Regulator,
        K::Scooter => GearType::Scooter,
        K::Suit => GearType::Wetsuit,
        K::Tank => GearType::Cylinder,
        K::Compressor | K::Lead | K::VariousPieces | K::Watch => GearType::Other,
        _ => return None,
    })
}

/// One piece of the owner's kit, with what its subtype adds.
struct Piece<'a> {
    kind: u::EquipmentKind,
    element: &'a u::EquipmentPiece,
    suit_type: Option<&'a u::SuitType>,
    tank: Option<&'a u::Tank>,
}

/// The owner's kit in `equipmentType`'s declaration order.
fn pieces(equipment: &u::Equipment) -> Vec<Piece<'_>> {
    use u::EquipmentKind as K;
    fn plain(kind: u::EquipmentKind, list: &[u::EquipmentPiece]) -> Vec<Piece<'_>> {
        list.iter()
            .map(|piece| Piece {
                kind,
                element: piece,
                suit_type: None,
                tank: None,
            })
            .collect()
    }
    let mut out = Vec::new();
    for (kind, list) in [
        (K::Boots, &equipment.boots),
        (
            K::BuoyancyControlDevice,
            &equipment.buoyancy_control_devices,
        ),
        (K::Camera, &equipment.cameras),
        (K::Compass, &equipment.compasses),
        (K::Compressor, &equipment.compressors),
        (K::DiveComputer, &equipment.dive_computers),
        (K::Fins, &equipment.fins),
        (K::Gloves, &equipment.gloves),
        (K::Knife, &equipment.knives),
        (K::Lead, &equipment.leads),
        (K::Light, &equipment.lights),
        (K::Mask, &equipment.masks),
    ] {
        out.extend(plain(kind, list));
    }
    out.extend(equipment.rebreathers.iter().map(|rebreather| Piece {
        kind: K::Rebreather,
        element: &rebreather.piece,
        suit_type: None,
        tank: None,
    }));
    out.extend(plain(K::Regulator, &equipment.regulators));
    out.extend(plain(K::Scooter, &equipment.scooters));
    out.extend(equipment.suits.iter().map(|suit| Piece {
        kind: K::Suit,
        element: &suit.piece,
        suit_type: suit.suit_type.as_ref(),
        tank: None,
    }));
    out.extend(equipment.tanks.iter().map(|tank| Piece {
        kind: K::Tank,
        element: &tank.piece,
        suit_type: None,
        tank: Some(tank),
    }));
    out.extend(plain(K::VariousPieces, &equipment.various_pieces));
    out.extend(plain(K::VideoCamera, &equipment.video_cameras));
    out.extend(plain(K::Watch, &equipment.watches));
    out
}

fn carries_address(address: Option<&u::Address>) -> bool {
    address.is_some_and(|a| {
        [&a.street, &a.city, &a.postcode, &a.country, &a.province]
            .into_iter()
            .any(|part| text(part.as_ref()).is_some())
    })
}

fn carries_contact(contact: Option<&u::Contact>) -> bool {
    contact.is_some_and(|c| {
        [
            &c.languages,
            &c.phones,
            &c.mobile_phones,
            &c.faxes,
            &c.emails,
            &c.homepages,
        ]
        .into_iter()
        .any(|list| list.iter().any(|value| !value.trim().is_empty()))
    })
}

fn texts(list: &[String]) -> impl Iterator<Item = &str> {
    list.iter()
        .map(|value| value.trim())
        .filter(|v| !v.is_empty())
}

impl<'a> Reader<'a> {
    fn new(source: &'a u::Uddf) -> Self {
        let generator = &source.generator;
        let is_shearwater = text(generator.name.as_ref()) == Some(SHEARWATER.0)
            && generator
                .manufacturer
                .as_ref()
                .and_then(|m| m.id.as_deref())
                == Some(SHEARWATER.1);
        Self {
            source,
            refs: source.references(),
            notes: Vec::new(),
            claimed: HashSet::new(),
            site_uuids: HashMap::new(),
            trip_uuids: HashMap::new(),
            gear_uuids: HashMap::new(),
            tank_volumes: HashMap::new(),
            computers: HashMap::new(),
            mixes: HashMap::new(),
            deco_models: HashMap::new(),
            contact_uuids: HashMap::new(),
            unread_contacts: HashSet::new(),
            contacts: Vec::new(),
            contact_names: HashMap::new(),
            person_uuids: HashMap::new(),
            unread_people: HashSet::new(),
            students: HashSet::new(),
            guides: HashMap::new(),
            linked_guides: HashSet::new(),
            local_clock_with_z: is_shearwater,
            percent_gradient_factors: is_shearwater,
            reported_gradient_scale: false,
        }
    }

    fn note(
        &mut self,
        at: &str,
        message: impl Into<String>,
        kind: NoteKind,
        code: &'static str,
        member: Option<String>,
    ) {
        self.notes.push(Note {
            kind,
            code: Some(code),
            at: at.to_owned(),
            member,
            message: message.into(),
        });
    }

    fn not_kept(&mut self, at: &str, said: &str, on: &str) {
        self.note(
            at,
            format!("{said} on {on} has no place in the logbook; it is left out."),
            NoteKind::Dropped,
            "detail_not_kept",
            None,
        );
    }

    /// Whether any element of the file carries this id.
    fn defined(&self, id: &str) -> bool {
        self.refs.get(id).is_some()
    }

    // -- identity ----------------------------------------------------------------------

    /// A record's stable UUID (`docs/converting.md`, *Identity*), or `None` where another
    /// record of this file already holds it.
    fn uuid_for(
        &mut self,
        kind: &str,
        source_id: Option<&str>,
        at: &str,
        index: &str,
    ) -> Option<Uuid> {
        let id = if let Some(id) = source_id {
            id.to_owned()
        } else {
            self.note(
                at,
                format!("The file gives this {kind} no identifier, so it is identified by its place in the file, which changes if the file's order does."),
                NoteKind::Absent,
                "identity_from_position",
                None,
            );
            format!("#{index}")
        };
        let derived = uuid::Uuid::new_v5(&ID_NAMESPACE, format!("{kind}:{id}").as_bytes());
        let first = embedded_uuid(&id).unwrap_or(derived);
        for candidate in [first, derived] {
            if self.claimed.insert(candidate) {
                return Some(Uuid(candidate));
            }
        }
        self.note(
            at,
            format!("A second {kind} carries the identifier {id:?}, which another record already has; it is left out, since two records cannot share one."),
            NoteKind::Dropped,
            "identifier_repeated",
            None,
        );
        None
    }

    // -- text --------------------------------------------------------------------------

    fn capped(&mut self, value: &str, limit: usize, at: &str, label: &str) -> String {
        let length = value.chars().count();
        if length <= limit {
            return value.to_owned();
        }
        self.note(
            at,
            format!(
                "{} is {length} characters long; the first {limit} are kept and the rest is left out.",
                told(label)
            ),
            NoteKind::Dropped,
            "text_shortened",
            None,
        );
        value.chars().take(limit).collect()
    }

    fn email(&mut self, value: Option<&str>, at: &str, member: Option<&str>) -> Option<String> {
        let value = value?;
        if !is_email(value) {
            self.note(
                at,
                format!("The email address {value:?} is not an address; it is left out."),
                NoteKind::Dropped,
                "email_unreadable",
                member.map(str::to_owned),
            );
            return None;
        }
        let length = value.chars().count();
        if length > MAX_EMAIL {
            self.note(
                at,
                format!("The email address is {length} characters long, more than the {MAX_EMAIL} kept, and a shortened address is a wrong one; it is left out."),
                NoteKind::Dropped,
                "email_too_long",
                member.map(str::to_owned),
            );
            return None;
        }
        Some(value.to_owned())
    }

    fn first_email(
        &mut self,
        contact: Option<&u::Contact>,
        at: &str,
        whose: &str,
    ) -> Option<String> {
        let recorded: Vec<&str> = contact
            .map(|c| texts(&c.emails).collect())
            .unwrap_or_default();
        for value in recorded.iter().skip(1) {
            self.note(
                at,
                format!(
                    "One email address is kept, the first {whose} records; {value:?} is left out."
                ),
                NoteKind::Dropped,
                "email_beyond_first",
                None,
            );
        }
        self.email(recorded.first().copied(), at, None)
    }

    /// The first `<phone>`, else the first `<mobilephone>`, reporting every other one.
    fn phone(
        &mut self,
        contact: Option<&u::Contact>,
        at: &str,
        whose: &str,
        member: Option<&str>,
    ) -> Option<String> {
        let contact = contact?;
        let recorded: Vec<(&str, &str)> = texts(&contact.phones)
            .map(|v| ("phone", v))
            .chain(texts(&contact.mobile_phones).map(|v| ("mobilephone", v)))
            .collect();
        for (tag, value) in recorded.iter().skip(1) {
            self.note(
                at,
                format!("One phone number is kept, the first {whose} records; <{tag}> {value:?} is left out."),
                NoteKind::Dropped,
                "phone_beyond_first",
                member.map(str::to_owned),
            );
        }
        let (_, value) = *recorded.first()?;
        let length = value.chars().count();
        if length > MAX_PHONE {
            self.note(
                at,
                format!("The phone number is {length} characters long, more than the {MAX_PHONE} kept, and a shortened number is a wrong one; it is left out."),
                NoteKind::Dropped,
                "phone_too_long",
                member.map(str::to_owned),
            );
            return None;
        }
        Some(value.to_owned())
    }

    fn website(&mut self, contact: Option<&u::Contact>, at: &str) -> Option<String> {
        let recorded: Vec<&str> = contact
            .map(|c| texts(&c.homepages).collect())
            .unwrap_or_default();
        for value in recorded.iter().skip(1) {
            self.note(
                at,
                format!(
                    "One website is kept, the first the contact records; {value:?} is left out."
                ),
                NoteKind::Dropped,
                "website_beyond_first",
                None,
            );
        }
        let value = *recorded.first()?;
        if !is_absolute_uri(value) {
            self.note(
                at,
                format!("The homepage {value:?} is not a full web address with its scheme; it is left out rather than given one the file never stated."),
                NoteKind::Dropped,
                "website_unreadable",
                None,
            );
            return None;
        }
        let length = value.chars().count();
        if length > MAX_WEBSITE {
            self.note(
                at,
                format!("The homepage is {length} characters long, more than the {MAX_WEBSITE} kept, and a shortened address is a wrong one; it is left out."),
                NoteKind::Dropped,
                "website_too_long",
                None,
            );
            return None;
        }
        Some(value.to_owned())
    }

    fn address(&mut self, address: Option<&u::Address>, at: &str) -> Option<Address> {
        if !carries_address(address) {
            return None;
        }
        let address = address?;
        let Some(country) = text(address.country.as_ref()) else {
            self.note(
                at,
                "The address records no <country>, which every other part of it is read inside; the address is left out.",
                NoteKind::Dropped,
                "address_without_country",
                None,
            );
            return None;
        };
        let mut part = |value: Option<&String>, cap, member: &str| {
            text(value).map(|v| self.capped(v, cap, at, &format!("the address's {member}")))
        };
        let street = part(address.street.as_ref(), MAX_NAME, "street");
        let city = part(address.city.as_ref(), MAX_NAME, "city");
        let postcode = part(address.postcode.as_ref(), MAX_POSTCODE, "postcode");
        let region = part(address.province.as_ref(), MAX_NAME, "region");
        let country = self.capped(country, MAX_NAME, at, "the address's country");
        let mut out = Address::new(country);
        out.street = street;
        out.city = city;
        out.postcode = postcode;
        out.region = region;
        Some(out)
    }

    fn position(
        &mut self,
        geography: Option<&u::Geography>,
        at: &str,
        member: Option<String>,
    ) -> Option<Position> {
        let latitude = finite(geography.and_then(|g| g.latitude));
        let longitude = finite(geography.and_then(|g| g.longitude));
        let (Some(latitude), Some(longitude)) = (latitude, longitude) else {
            if latitude.is_some() || longitude.is_some() {
                self.note(
                    at,
                    "Only one half of a coordinate pair is recorded, and a position needs both; it is left out.",
                    NoteKind::Dropped,
                    "position_incomplete",
                    member,
                );
            }
            return None;
        };
        if latitude == 0.0 && longitude == 0.0 {
            self.note(
                at,
                "The coordinates are exactly 0 / 0, which files use to mean 'unknown', so no position is recorded.",
                NoteKind::Absent,
                "position_zero",
                member,
            );
            return None;
        }
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            self.note(
                at,
                format!("The coordinates {latitude} / {longitude} are not a place on Earth; they are left out."),
                NoteKind::Dropped,
                "position_out_of_range",
                member,
            );
            return None;
        }
        Some(Position::new(latitude, longitude))
    }

    fn altitude(&mut self, value: Option<f64>, at: &str, member: String) -> Option<i32> {
        let altitude = rounded(finite(value)?)?;
        if (-450..=6500).contains(&altitude) {
            return i32::try_from(altitude).ok();
        }
        self.note(
            at,
            format!("<altitude> is {altitude} m, outside the -450 to 6500 m a dive can be at; it is left out."),
            NoteKind::Dropped,
            "altitude_out_of_range",
            Some(member),
        );
        None
    }

    // -- the run -----------------------------------------------------------------------

    fn run(&mut self, exported_at: DateTime) -> Document {
        self.read_mixes();
        self.read_deco_models();
        self.read_guides();
        let sites = self.read_sites();
        self.read_contacts();
        let trips = self.read_trips();
        let gear = self.read_gear();
        let people = self.read_people();
        let dives = self.read_dives();
        let owner = self.read_diver();

        let mut doc = Document::new(exported_at);
        let mut generator = Generator::new("divejson");
        generator.version = Some(env!("CARGO_PKG_VERSION").to_owned());
        doc.generator = Some(generator);
        doc.diver = owner;
        doc.dives = dives;
        doc.trips = trips;
        doc.sites = sites;
        doc.gear = gear;
        doc.contacts = core::mem::take(&mut self.contacts);
        doc.people = people;
        doc.extensions
            .insert("divejson".to_owned(), self.provenance());
        doc
    }

    fn provenance(&self) -> serde_json::Value {
        let mut provenance = serde_json::Map::new();
        provenance.insert("converted_from".to_owned(), "uddf".into());
        if let Some(version) = text(self.source.version.as_ref()) {
            provenance.insert("uddf_version".to_owned(), version.into());
        }
        let generator = &self.source.generator;
        if let Some(name) = text(generator.name.as_ref()) {
            let mut source = serde_json::Map::new();
            source.insert("name".to_owned(), name.into());
            if let Some(version) = text(generator.version.as_ref()) {
                source.insert("version".to_owned(), version.into());
            }
            provenance.insert("source_generator".to_owned(), source.into());
        }
        provenance.into()
    }

    // -- diver -------------------------------------------------------------------------

    fn read_diver(&mut self) -> Option<Diver> {
        let source = self.source;
        let owner = source.owner.as_ref()?;
        let at = "diver";
        let personal = &owner.personal;
        let names: Vec<&str> = [
            &personal.first_name,
            &personal.middle_name,
            &personal.last_name,
        ]
        .into_iter()
        .filter_map(|part| text(part.as_ref()))
        .collect();
        let contact = owner.contact.as_ref();
        let first_email = contact.and_then(|c| texts(&c.emails).next());
        let email = self.email(first_email, at, Some("diver/email"));
        let phone = self.phone(contact, at, "the owner", Some("diver/phone"));
        let born_on = date_of(personal.birth_date.as_ref());
        let insurances = self.read_insurances(owner, at);
        if names.is_empty()
            && email.is_none()
            && phone.is_none()
            && born_on.is_none()
            && insurances.is_empty()
        {
            self.note(
                at,
                "The file records nothing about the logbook's owner, so no diver is written.",
                NoteKind::Absent,
                "owner_not_recorded",
                Some("diver".to_owned()),
            );
            return None;
        }
        let uuid = self.uuid_for("diver", owner.id.as_deref(), at, "0");
        let name = (!names.is_empty())
            .then(|| self.capped(&names.join(" "), MAX_NAME, at, "the diver's name"));
        Some(Diver {
            uuid,
            name,
            email,
            phone,
            born_on,
            insurances,
            ..Diver::default()
        })
    }

    fn read_insurances(&mut self, owner: &u::Person, at: &str) -> Vec<Insurance> {
        let mut out = Vec::new();
        for (index, element) in owner.insurances.iter().enumerate() {
            let here = format!("{at}/insurance/{index}");
            let Some(provider) = text(element.name.as_ref()) else {
                self.note(
                    &here,
                    "An insurance has no insurer's name, and one cannot be kept without it; it is left out.",
                    NoteKind::Dropped,
                    "insurance_unnamed",
                    None,
                );
                continue;
            };
            let mut insurance =
                Insurance::new(self.capped(provider, MAX_NAME, &here, "the insurer's name"));
            insurance.expires_on = date_of(element.valid_date.as_ref());
            let mut unread: Vec<String> = texts(&element.alias_names)
                .map(|alias| format!("<aliasname> {alias:?}"))
                .collect();
            if element.issue_date.is_some() {
                unread.push("<issuedate>".to_owned());
            }
            if notes_text(&element.notes).is_some() {
                unread.push("<notes>".to_owned());
            }
            for what in unread {
                self.note(
                    &here,
                    format!("An insurance's {what} has no place in the logbook; it is left out."),
                    NoteKind::Dropped,
                    "detail_not_kept",
                    None,
                );
            }
            out.push(insurance);
        }
        out
    }

    // -- people ------------------------------------------------------------------------

    fn read_people(&mut self) -> Vec<Person> {
        let source = self.source;
        let mut people = Vec::new();
        for (index, buddy) in source.buddies.iter().enumerate() {
            let at = format!("buddy/{index}");
            let id = buddy.person.id.as_deref();
            let personal = &buddy.person.personal;
            let names: Vec<&str> = [
                &personal.first_name,
                &personal.middle_name,
                &personal.last_name,
            ]
            .into_iter()
            .filter_map(|part| text(part.as_ref()))
            .collect();
            if names.is_empty() {
                self.note(
                    &at,
                    "The buddy has no name, and a person cannot be kept without one; it is left out with every link to it.",
                    NoteKind::Dropped,
                    "person_unnamed",
                    None,
                );
                if let Some(id) = id {
                    self.unread_people.insert(id);
                }
                continue;
            }
            let Some(uuid) = self.uuid_for("person", id, &at, &index.to_string()) else {
                if let Some(id) = id {
                    self.unread_people.insert(id);
                }
                continue;
            };
            if let Some(id) = id {
                self.person_uuids.insert(id, uuid);
                if buddy.student {
                    self.students.insert(id);
                }
            }
            people.push(self.person(buddy, uuid, &names.join(" "), &at));
        }
        people
    }

    fn person(&mut self, buddy: &u::Buddy, uuid: Uuid, name: &str, at: &str) -> Person {
        let person = &buddy.person;
        let contact = person.contact.as_ref();
        let mut out = Person::new(uuid, self.capped(name, MAX_NAME, at, "the person's name"));
        out.email = self.first_email(contact, at, "the person");
        out.phone = self.phone(contact, at, "the person", None);
        out.notes = notes_text(&person.notes);
        let personal = &person.personal;
        let residue = [
            (carries_address(person.address.as_ref()), "<address>"),
            (!person.equipment.is_empty(), "<equipment>"),
            (!person.certifications.is_empty(), "<education>"),
            (!person.insurances.is_empty(), "<diveinsurances>"),
            (
                text(personal.birth_name.as_ref()).is_some(),
                "<personal><birthname>",
            ),
            (
                text(personal.honorific.as_ref()).is_some(),
                "<personal><honorific>",
            ),
            (personal.sex.is_some(), "<personal><sex>"),
            (personal.birth_date.is_some(), "<personal><birthdate>"),
            (
                text(personal.passport.as_ref()).is_some(),
                "<personal><passport>",
            ),
            (personal.membership.is_some(), "<personal><membership>"),
            (personal.height.is_some(), "<personal><height>"),
            (personal.weight.is_some(), "<personal><weight>"),
            (
                text(personal.blood_group.as_ref()).is_some(),
                "<personal><bloodgroup>",
            ),
            (
                text(personal.smoking.as_ref()).is_some(),
                "<personal><smoking>",
            ),
            (
                contact.is_some_and(|c| texts(&c.languages).next().is_some()),
                "<contact><language>",
            ),
            (
                contact.is_some_and(|c| texts(&c.faxes).next().is_some()),
                "<contact><fax>",
            ),
            (
                contact.is_some_and(|c| texts(&c.homepages).next().is_some()),
                "<contact><homepage>",
            ),
        ];
        for (present, said) in residue {
            if present {
                self.not_kept(at, said, "a buddy");
            }
        }
        out
    }

    fn read_guides(&mut self) {
        let source = self.source;
        for base in &source.dive_bases {
            for guide in &base.guides {
                if let Some(id) = guide.id.as_deref() {
                    self.guides.insert(id, guide.link.as_deref());
                }
            }
        }
        for dive in source.dives() {
            for link in &dive.information_before_dive.links {
                if let Some((id, _)) = self.guides.get_key_value(link.as_str()) {
                    self.linked_guides.insert(id);
                }
            }
        }
    }

    // -- sites -------------------------------------------------------------------------

    fn read_sites(&mut self) -> Vec<DiveSite> {
        let source = self.source;
        let mut sites = Vec::new();
        for (index, element) in source.sites.iter().enumerate() {
            let at = format!("site/{index}");
            let Some(name) = text(element.name.as_ref()) else {
                self.note(
                    &at,
                    "The site has no name, and a site cannot be logged without one; it is left out with every dive's link to it, since a name cannot be made up.",
                    NoteKind::Dropped,
                    "site_unnamed",
                    None,
                );
                continue;
            };
            let id = element.id.as_deref();
            let Some(uuid) = self.uuid_for("site", id, &at, &index.to_string()) else {
                continue;
            };
            if let Some(id) = id {
                self.site_uuids.insert(id, uuid);
            }
            let path = format!("sites/{}", sites.len());
            let mut site = DiveSite::new(uuid, self.capped(name, MAX_NAME, &at, "the site name"));
            let mut seen = HashSet::from([folded(&site.name)]);
            for alias in texts(&element.alias_names) {
                let alias = self.capped(alias, MAX_NAME, &at, "the site's other name");
                if seen.insert(folded(&alias)) {
                    site.other_names.push(alias);
                }
            }
            let geography = element.geography.as_ref();
            if let Some(location) = text(geography.and_then(|g| g.location.as_ref())) {
                site.location = Some(Location::new(self.capped(
                    location,
                    MAX_NAME,
                    &at,
                    "the site's locality",
                )));
            }
            site.position = self.position(geography, &at, Some(format!("{path}/position")));
            site.altitude = self.altitude(
                geography.and_then(|g| g.altitude),
                &at,
                format!("{path}/altitude"),
            );
            self.depth_range(element.site_data.as_ref(), &mut site, &at, &path);
            site.notes = notes_text(&element.notes);
            self.report_site_residue(element, &at);
            sites.push(site);
        }
        sites
    }

    fn depth_range(
        &mut self,
        data: Option<&u::SiteData>,
        site: &mut DiveSite,
        at: &str,
        path: &str,
    ) {
        let mut depth = |value: Option<f64>, tag: &str, member: &str| {
            let value = finite(value)?;
            if value < 0.0 {
                self.note(
                    at,
                    format!("<{tag}> is {value} m, above the surface; it is left out."),
                    NoteKind::Dropped,
                    "site_depth_above_surface",
                    Some(format!("{path}/{member}")),
                );
                return None;
            }
            Some(value)
        };
        let shallow = depth(
            data.and_then(|d| d.minimum_depth),
            "minimumdepth",
            "depth_from",
        );
        let deep = depth(
            data.and_then(|d| d.maximum_depth),
            "maximumdepth",
            "depth_to",
        );
        if let (Some(shallow), Some(deep)) = (shallow, deep)
            && shallow > deep
        {
            self.note(
                at,
                format!("The <minimumdepth> {shallow} m is deeper than the <maximumdepth> {deep} m, which cannot be; both are left out rather than either being adjusted to fit."),
                NoteKind::Dropped,
                "site_depths_inverted",
                Some(path.to_owned()),
            );
            return;
        }
        site.depth_from = shallow;
        site.depth_to = deep;
    }

    fn report_site_residue(&mut self, element: &u::Site, at: &str) {
        let geography = element.geography.as_ref();
        let data = element.site_data.as_ref();
        let residue = [
            (element.environment.is_some(), "<environment>"),
            (!element.ratings.is_empty(), "<rating>"),
            (
                geography.is_some_and(|g| carries_address(g.address.as_ref())),
                "<geography><address>",
            ),
            (
                geography.is_some_and(|g| g.timezone.is_some()),
                "<geography><timezone>",
            ),
            (
                data.is_some_and(|d| d.area_length.is_some()),
                "<sitedata><arealength>",
            ),
            (
                data.is_some_and(|d| d.area_width.is_some()),
                "<sitedata><areawidth>",
            ),
            (
                data.is_some_and(|d| d.average_visibility.is_some()),
                "<sitedata><averagevisibility>",
            ),
            (
                data.is_some_and(|d| d.bottom.is_some()),
                "<sitedata><bottom>",
            ),
            (
                data.is_some_and(|d| d.density.is_some()),
                "<sitedata><density>",
            ),
            (
                data.is_some_and(|d| d.difficulty.is_some()),
                "<sitedata><difficulty>",
            ),
            (
                data.is_some_and(|d| d.global_light_intensity.is_some()),
                "<sitedata><globallightintensity>",
            ),
            (
                data.is_some_and(|d| d.maximum_visibility.is_some()),
                "<sitedata><maximumvisibility>",
            ),
            (
                data.is_some_and(|d| d.minimum_visibility.is_some()),
                "<sitedata><minimumvisibility>",
            ),
            (
                data.is_some_and(|d| d.terrain.is_some()),
                "<sitedata><terrain>",
            ),
        ];
        for (present, said) in residue {
            if present {
                self.not_kept(at, said, "a site");
            }
        }
    }

    // -- contacts ----------------------------------------------------------------------

    fn read_contacts(&mut self) {
        let source = self.source;
        let mut linked: HashSet<&str> = source
            .dives()
            .flat_map(|dive| dive.information_before_dive.links.iter())
            .map(String::as_str)
            .collect();
        let mut named = HashSet::new();
        for part in source.trips.iter().flat_map(|trip| trip.parts.iter()) {
            if let Some(link) = part.link.as_deref() {
                linked.insert(link);
            }
            let stays = [
                part.accommodation.as_ref().and_then(|a| a.name.as_ref()),
                part.operator.as_ref().and_then(|o| o.name.as_ref()),
            ];
            for name in stays.into_iter().filter_map(text) {
                named.insert(folded(name));
            }
        }
        for (index, base) in source.dive_bases.iter().enumerate() {
            let at = format!("divebase/{index}");
            let id = base.id.as_deref();
            let carries = carries_address(base.address.as_ref())
                || carries_contact(base.contact.as_ref())
                || notes_text(&base.notes).is_some();
            if let Some(name) = text(base.name.as_ref())
                && !carries
                && !id.is_some_and(|id| linked.contains(id))
                && !named.contains(&folded(name))
            {
                self.note(
                    &at,
                    format!("The <divebase> {name:?} records nothing but its name and nothing in the file links it, which is how an application fills the place of a dive base it does not have; it is not kept as a contact."),
                    NoteKind::Dropped,
                    "placeholder_dive_base",
                    None,
                );
                if let Some(id) = id {
                    self.unread_contacts.insert(id);
                }
                continue;
            }
            let mut residue: Vec<String> = texts(&base.alias_names)
                .map(|alias| format!("<aliasname> {alias:?}"))
                .collect();
            for (present, said) in [
                (base.price_per_dive.is_some(), "<priceperdive>"),
                (base.price_dive_package.is_some(), "<pricedivepackage>"),
            ] {
                if present {
                    residue.push(said.to_owned());
                }
            }
            for guide in &base.guides {
                if !guide
                    .id
                    .as_deref()
                    .is_some_and(|id| self.linked_guides.contains(id))
                {
                    residue.push("<guide>".to_owned());
                }
            }
            residue.extend(base.ratings.iter().map(|_| "<rating>".to_owned()));
            if base.link.is_some() {
                residue.push("<link>".to_owned());
            }
            let shape = ShapeOf {
                tag: "divebase",
                id,
                name: text(base.name.as_ref()),
                address: base.address.as_ref(),
                contact: base.contact.as_ref(),
                notes: &base.notes,
                residue,
            };
            self.read_contact(&shape, ContactRole::DiveCenter, &at);
        }
        for (index, shop) in source.shops.iter().enumerate() {
            let shape = Self::shop_shape(shop);
            self.read_contact(&shape, ContactRole::Shop, &format!("shop/{index}"));
        }
    }

    fn shop_shape(shop: &u::Shop) -> ShapeOf<'_> {
        ShapeOf {
            tag: "shop",
            id: shop.id.as_deref(),
            name: text(shop.name.as_ref()),
            address: shop.address.as_ref(),
            contact: shop.contact.as_ref(),
            notes: &shop.notes,
            residue: texts(&shop.alias_names)
                .map(|alias| format!("<aliasname> {alias:?}"))
                .collect(),
        }
    }

    fn read_contact(&mut self, shape: &ShapeOf<'a>, role: ContactRole, at: &str) {
        let Some(name) = shape.name else {
            self.note(
                at,
                format!("The <{}> has no name, and a contact cannot be kept without one; it is left out with every link to it.", shape.tag),
                NoteKind::Dropped,
                "contact_unnamed",
                None,
            );
            if let Some(id) = shape.id {
                self.unread_contacts.insert(id);
            }
            return;
        };
        let Some(uuid) = self.uuid_for("contact", shape.id, at, at) else {
            if let Some(id) = shape.id {
                self.unread_contacts.insert(id);
            }
            return;
        };
        if let Some(id) = shape.id {
            self.contact_uuids.insert(id, uuid);
        }
        self.add_contact(shape, uuid, name, role, at);
    }

    fn add_contact(
        &mut self,
        shape: &ShapeOf<'_>,
        uuid: Uuid,
        name: &str,
        role: ContactRole,
        at: &str,
    ) {
        let mut contact = Contact::new(uuid, self.capped(name, MAX_NAME, at, "the contact's name"));
        contact.roles = vec![role];
        let contents = self.contact_contents(shape, at);
        contact.phone = contents.phone;
        contact.email = contents.email;
        contact.website = contents.website;
        contact.address = contents.address;
        contact.notes = contents.notes;
        self.contact_names
            .entry(folded(name))
            .or_insert(self.contacts.len());
        self.contacts.push(contact);
    }

    fn contact_contents(&mut self, shape: &ShapeOf<'_>, at: &str) -> Shape {
        let contact = shape.contact;
        let out = Shape {
            phone: self.phone(contact, at, "the contact", None),
            email: self.first_email(contact, at, "the contact"),
            website: self.website(contact, at),
            address: self.address(shape.address, at),
            notes: notes_text(shape.notes),
        };
        for said in &shape.residue {
            self.not_kept(at, said, "a contact");
        }
        if let Some(contact) = contact {
            for (list, said) in [
                (&contact.languages, "<contact><language>"),
                (&contact.faxes, "<contact><fax>"),
            ] {
                if texts(list).next().is_some() {
                    self.not_kept(at, said, "a contact");
                }
            }
        }
        out
    }

    /// A shape held inside another element — a part's stay, a purchase's shop — folded by
    /// name into a contact already read, or a contact of its own.
    fn read_inline(
        &mut self,
        shape: &ShapeOf<'_>,
        role: ContactRole,
        at: &str,
        position: &str,
    ) -> Option<Uuid> {
        let Some(name) = shape.name else {
            self.note(
                at,
                format!(
                    "The <{}> has no name, and a contact cannot be kept without one; it is left out.",
                    shape.tag
                ),
                NoteKind::Dropped,
                "contact_unnamed",
                None,
            );
            return None;
        };
        if let Some(&index) = self.contact_names.get(&folded(name)) {
            let contents = self.contact_contents(shape, at);
            let mut differing = Vec::new();
            let standing = self.contacts.get_mut(index)?;
            if !standing.roles.contains(&role) {
                standing.roles.push(role);
                standing.roles.sort_by_key(|role| role_rank(*role));
            }
            fill(&mut standing.phone, contents.phone, "phone", &mut differing);
            fill(&mut standing.email, contents.email, "email", &mut differing);
            fill(
                &mut standing.website,
                contents.website,
                "website",
                &mut differing,
            );
            fill(
                &mut standing.address,
                contents.address,
                "address",
                &mut differing,
            );
            fill(&mut standing.notes, contents.notes, "notes", &mut differing);
            let (uuid, standing_name) = (standing.uuid, standing.name.clone());
            for member in differing {
                self.note(
                    at,
                    format!("The <{}> is the contact {standing_name:?} by name, and gives its {member} differently; the contact's is kept.", shape.tag),
                    NoteKind::Dropped,
                    "contact_details_differ",
                    None,
                );
            }
            return Some(uuid);
        }
        let uuid = self.uuid_for("contact", shape.id, at, position)?;
        self.add_contact(shape, uuid, name, role, at);
        Some(uuid)
    }

    // -- trips -------------------------------------------------------------------------

    fn read_trips(&mut self) -> Vec<Trip> {
        let source = self.source;
        let mut trips = Vec::new();
        let mut first_part = 0_usize;
        for (index, element) in source.trips.iter().enumerate() {
            let at = format!("trip/{index}");
            let first = first_part;
            first_part = first_part.saturating_add(element.parts.len());
            let Some(name) = text(element.name.as_ref()) else {
                self.note(
                    &at,
                    "The trip has no name, and a trip cannot be kept without one; it is left out.",
                    NoteKind::Dropped,
                    "trip_unnamed",
                    None,
                );
                continue;
            };
            let id = element.id.as_deref();
            let Some(uuid) = self.uuid_for("trip", id, &at, &index.to_string()) else {
                continue;
            };
            if let Some(id) = id {
                self.trip_uuids.insert(id, uuid);
            }
            let mut trip = Trip::new(uuid, self.capped(name, MAX_NAME, &at, "the trip name"));
            let mut paragraphs = Vec::new();
            for (part_index, part) in element.parts.iter().enumerate() {
                let part_at = format!("{at}/trippart/{part_index}");
                let position = first.saturating_add(part_index).to_string();
                if let Some(record) = self.read_trip_part(part, &part_at, &position) {
                    trip.parts.push(record);
                }
                if let Some(notes) = notes_text(&part.notes) {
                    paragraphs.push(notes);
                }
            }
            if !paragraphs.is_empty() {
                trip.notes = Some(paragraphs.join("\n\n"));
            }
            trips.push(trip);
        }
        trips
    }

    fn read_trip_part(&mut self, part: &u::TripPart, at: &str, position: &str) -> Option<TripPart> {
        let stay = self.read_stay(part, at, position);
        if let Some(kind) = &part.kind {
            self.not_kept(at, &format!("The type {:?}", kind.as_str()), "a trip part");
        }
        if let Some(link) = part.link.as_deref() {
            let message = if self.contact_uuids.contains_key(link) {
                format!(
                    "The part links the contact {link:?}, saying the diver dived with it during the part; a trip part records where the diver stayed, and whom a dive was with belongs to the dive, so the link is left out."
                )
            } else {
                format!(
                    "The part links {link:?}, which a trip part has no place for; the link is left out."
                )
            };
            self.note(
                at,
                message,
                NoteKind::Dropped,
                "trip_part_link_not_kept",
                None,
            );
        }
        let starts = date_of(part.start_date.as_ref());
        let mut ends = date_of(part.end_date.as_ref());
        if let (Some(starts), Some(end)) = (starts, ends)
            && end < starts
        {
            self.note(
                at,
                format!("The part ends on {end}, before it starts on {starts}; the end date is left out."),
                NoteKind::Dropped,
                "trip_part_ends_before_start",
                None,
            );
            ends = None;
        }
        let geography = part.geography.as_ref();
        let part_name = text(part.name.as_ref());
        let place = text(geography.and_then(|g| g.location.as_ref()));
        let mut location = None;
        if let Some(place) = place {
            location = Some(Location::new(self.capped(
                place,
                MAX_NAME,
                at,
                "the trip part's location",
            )));
            if let Some(part_name) = part_name
                && part_name != place
            {
                self.note(
                    at,
                    format!("The part's <name> {part_name:?} differs from its <geography><location>, which is the place's name; a trip part keeps no name of its own, so it is left out."),
                    NoteKind::Dropped,
                    "trip_part_name_not_kept",
                    None,
                );
            }
        } else if let Some(part_name) = part_name {
            location = Some(Location::new(self.capped(
                part_name,
                MAX_NAME,
                at,
                "the trip part's name",
            )));
        }
        if let Some(location) = &mut location {
            location.position = self.position(geography, at, None);
        } else if geography.is_some() {
            let kept = if starts.is_some() || ends.is_some() || stay.is_some() {
                "the rest of the part is kept"
            } else {
                "the part records no dates and no stay either, so nothing of it is kept"
            };
            self.note(
                at,
                format!("The trip part names no place in its <name> or its <geography><location>, and a place cannot be kept without a name; the place is left out and {kept}."),
                NoteKind::Dropped,
                "trip_part_place_unnamed",
                None,
            );
        }
        if starts.is_none() && ends.is_none() && location.is_none() && stay.is_none() {
            return None;
        }
        Some(TripPart {
            starts_on: starts,
            ends_on: ends,
            location,
            accommodation_uuid: stay,
            ..TripPart::default()
        })
    }

    fn read_stay(&mut self, part: &u::TripPart, at: &str, position: &str) -> Option<Uuid> {
        if let Some(vessel) = &part.vessel {
            let named = text(vessel.name.as_ref())
                .map(|name| format!(" {name:?}"))
                .unwrap_or_default();
            self.note(
                &format!("{at}/vessel"),
                format!("The <vessel>{named} is a boat, which the logbook does not keep; it and everything it records about the boat are left out, the operator being the contact."),
                NoteKind::Dropped,
                "vessel_not_kept",
                None,
            );
        }
        if let Some(stay) = &part.accommodation {
            let mut residue: Vec<String> = texts(&stay.alias_names)
                .map(|alias| format!("<aliasname> {alias:?}"))
                .collect();
            if stay.category.is_some() {
                residue.push("<category>".to_owned());
            }
            residue.extend(stay.ratings.iter().map(|_| "<rating>".to_owned()));
            let shape = ShapeOf {
                tag: "accomodation",
                id: stay.id.as_deref(),
                name: text(stay.name.as_ref()),
                address: stay.address.as_ref(),
                contact: stay.contact.as_ref(),
                notes: &stay.notes,
                residue,
            };
            return self.read_inline(
                &shape,
                ContactRole::Accommodation,
                &format!("{at}/accomodation"),
                position,
            );
        }
        if let Some(operator) = &part.operator {
            let mut residue: Vec<String> = texts(&operator.alias_names)
                .map(|alias| format!("<aliasname> {alias:?}"))
                .collect();
            residue.extend(operator.ratings.iter().map(|_| "<rating>".to_owned()));
            let shape = ShapeOf {
                tag: "operator",
                id: None,
                name: text(operator.name.as_ref()),
                address: operator.address.as_ref(),
                contact: operator.contact.as_ref(),
                notes: &operator.notes,
                residue,
            };
            return self.read_inline(
                &shape,
                ContactRole::Liveaboard,
                &format!("{at}/operator"),
                position,
            );
        }
        None
    }

    // -- gear --------------------------------------------------------------------------

    fn read_gear(&mut self) -> Vec<GearItem> {
        let source = self.source;
        let Some(owner) = source.owner.as_ref() else {
            return Vec::new();
        };
        let mut gear = Vec::new();
        for (index, item) in pieces(&owner.equipment).into_iter().enumerate() {
            let Some(mut gear_type) = gear_type(item.kind) else {
                continue;
            };
            let at = format!("gear/{index}");
            let element = item.element;
            let id = element.id.as_deref();
            if item.kind == u::EquipmentKind::DiveComputer
                && let Some(id) = id
            {
                self.computers.insert(id, element);
            }
            let Some(name) = text(element.name.as_ref()) else {
                self.note(
                    &at,
                    format!("The <{}> has no name, and a piece of gear cannot be kept without one; it is left out.", item.kind.element()),
                    NoteKind::Dropped,
                    "gear_unnamed",
                    None,
                );
                continue;
            };
            let Some(uuid) = self.uuid_for("gear", id, &at, &index.to_string()) else {
                continue;
            };
            let volume = item.tank.and_then(|tank| self.tank_volume(tank, &at));
            if let Some(id) = id {
                self.gear_uuids.insert(id, uuid);
                if let Some(volume) = volume {
                    self.tank_volumes.insert(id, volume);
                }
            }
            if let Some(purchase) = &element.purchase {
                self.read_purchase(purchase, &format!("{at}/purchase/0"));
            }
            let mut out = GearItem::new(uuid, self.capped(name, MAX_NAME, &at, "the gear name"));
            if let Some(brand) = text(element.manufacturer.as_ref().and_then(|m| m.name.as_ref())) {
                out.brand = Some(self.capped(brand, MAX_NAME, &at, "the brand"));
            }
            if let Some(serial) = text(element.serial_number.as_ref()) {
                out.serial = Some(self.capped(serial, MAX_SERIAL, &at, "the serial number"));
            }
            if item.suit_type.is_some_and(|suit| {
                matches!(
                    suit.as_str().to_lowercase().as_str(),
                    "dry-suit" | "drysuit" | "hot-water-suit"
                )
            }) {
                gear_type = GearType::Drysuit;
            }
            out.gear_type = Some(gear_type);
            out.notes = notes_text(&element.notes);
            if let Some(tank) = item.tank {
                out.volume = volume;
                out.material = self.tank_material(tank, &at);
            }
            gear.push(out);
        }
        gear
    }

    fn tank_volume(&mut self, tank: &u::Tank, at: &str) -> Option<f64> {
        let raw = finite(tank.volume)?;
        let (litres, reinterpreted) = volume_litres(raw);
        if litres <= 0.0 {
            self.size_zero(at, litres, None);
            return None;
        }
        if reinterpreted {
            self.size_as_litres(at, raw, litres, None);
        }
        Some(litres)
    }

    fn size_zero(&mut self, at: &str, litres: f64, member: Option<String>) {
        self.note(
            at,
            format!(
                "<tankvolume> reads as {litres} litres, which is no size; read as not recorded."
            ),
            NoteKind::Absent,
            "cylinder_size_zero",
            member,
        );
    }

    fn size_as_litres(&mut self, at: &str, raw: f64, litres: f64, member: Option<String>) {
        self.note(
            at,
            format!("<tankvolume> is {raw}, too large to be the cubic metres UDDF specifies; read as {litres} litres, which is how some versions of Subsurface write it."),
            NoteKind::Resolved,
            "cylinder_size_read_as_litres",
            member,
        );
    }

    fn tank_material(&mut self, tank: &u::Tank, at: &str) -> Option<Material> {
        let material = tank.material.as_ref()?;
        match material {
            u::TankMaterial::Aluminium => Some(Material::Aluminium),
            u::TankMaterial::Carbon => Some(Material::Carbon),
            u::TankMaterial::Steel => Some(Material::Steel),
            other => {
                self.note(
                    at,
                    format!(
                        "The tank material {:?} is not one the logbook names; it is left out.",
                        other.as_str()
                    ),
                    NoteKind::Dropped,
                    "tank_material_unknown",
                    None,
                );
                None
            }
        }
    }

    fn read_purchase(&mut self, purchase: &u::Purchase, at: &str) {
        let shop_at = format!("{at}/shop");
        let read = purchase.shop.as_ref().is_some_and(|shop| {
            let shape = Self::shop_shape(shop);
            self.read_inline(&shape, ContactRole::Shop, &shop_at, &shop_at)
                .is_some()
        });
        let tail = if read {
            ", and the shop it names is kept as a contact."
        } else {
            "."
        };
        self.note(
            at,
            format!("A piece's <purchase>, its price and its date, has no place in the logbook and is left out{tail}"),
            NoteKind::Dropped,
            "purchase_not_kept",
            None,
        );
    }

    // -- gases -------------------------------------------------------------------------

    fn read_mixes(&mut self) {
        let source = self.source;
        for (index, element) in source.mixes.iter().enumerate() {
            let Some(id) = element.id.as_deref() else {
                continue;
            };
            let at = format!("mix/{index}");
            let mut gas = Gas::default();
            for (raw, tag) in [(element.o2, "o2"), (element.he, "he")] {
                let Some(raw) = finite(raw) else { continue };
                let (percent, reinterpreted) = gas_percent(raw);
                if reinterpreted {
                    self.note(
                        &at,
                        format!("<{tag}> is {raw}, above the 1.0 the UDDF documentation describes; read as {percent} percent rather than as a fraction."),
                        NoteKind::Resolved,
                        "gas_read_as_percent",
                        None,
                    );
                }
                if !(0.0..=100.0).contains(&percent) {
                    self.note(
                        &at,
                        format!("<{tag}> reads as {percent} percent, outside the 0 to 100 a mix can be; it is left out."),
                        NoteKind::Dropped,
                        "gas_percent_out_of_range",
                        None,
                    );
                    continue;
                }
                if tag == "o2" {
                    gas.oxygen = Some(percent);
                } else {
                    gas.helium = Some(percent);
                }
            }
            if gas.oxygen.unwrap_or(0.0) + gas.helium.unwrap_or(0.0) > 100.0 {
                self.note(
                    &at,
                    "Oxygen and helium add up to more than 100 percent, which no mix can; both are left out.",
                    NoteKind::Dropped,
                    "gas_over_100_percent",
                    None,
                );
                gas.oxygen = None;
                gas.helium = None;
            }
            if let Some(limit) = finite(element.maximum_po2) {
                if (0.4..=2.0).contains(&limit) {
                    gas.ppo2_limit = Some(limit);
                } else {
                    self.note(
                        &at,
                        format!("<maximumpo2> is {limit} bar, outside the 0.4 to 2.0 bar a limit can be; it is left out."),
                        NoteKind::Dropped,
                        "ppo2_limit_out_of_range",
                        None,
                    );
                }
            }
            self.mixes.insert(id, gas);
        }
    }

    fn read_deco_models(&mut self) {
        let source = self.source;
        for model in &source.deco_models {
            if let Some(id) = model.id() {
                self.deco_models.insert(id, model);
            }
        }
    }

    fn read_deco_model(
        &mut self,
        before: &u::InformationBeforeDive,
        at: &str,
        recording: &str,
    ) -> Option<DecoModel> {
        let model = before
            .links
            .iter()
            .find_map(|link| self.deco_models.get(link.as_str()).copied())?;
        let member = format!("{recording}/deco_model");
        let u::DecoModel::Buehlmann(model) = model else {
            let tag = match model {
                u::DecoModel::Vpm(_) => "vpm",
                _ => "rgbm",
            };
            self.note(
                at,
                format!("The dive's decompression model is a <{tag}>, which is not one that can be named; it is left out rather than read as another."),
                NoteKind::Dropped,
                "deco_model_unknown",
                Some(member),
            );
            return None;
        };
        let mut pair = Vec::new();
        for (value, tag) in [
            (model.gradient_factor_low, "<gradientfactorlow>"),
            (model.gradient_factor_high, "<gradientfactorhigh>"),
        ] {
            let Some(value) = self.gradient_factor(value) else {
                pair.push(None);
                continue;
            };
            if let Some(value) = u8::try_from(value).ok().filter(|v| *v <= 100) {
                pair.push(Some(value));
            } else {
                self.note(
                    at,
                    format!("{} is {value}, and a gradient factor is a whole percent from 0 to 100; it is left out.", told(tag)),
                    NoteKind::Dropped,
                    "gradient_factor_out_of_range",
                    Some(member.clone()),
                );
                pair.push(None);
            }
        }
        let mut out = DecoModel {
            algorithm: Some(DecoAlgorithm::Buhlmann),
            ..DecoModel::default()
        };
        match pair.as_slice() {
            [Some(low), Some(high)] if low > high => self.note(
                at,
                format!("<gradientfactorlow> is {low} and <gradientfactorhigh> is {high}, and the low cannot be above the high; both are left out, the file not saying which of the two is wrong."),
                NoteKind::Dropped,
                "gradient_factors_inverted",
                Some(member),
            ),
            [Some(low), Some(high)] => {
                out.gf_low = Some(*low);
                out.gf_high = Some(*high);
            }
            [Some(one), None] | [None, Some(one)] => self.note(
                at,
                format!("A gradient factor is {one} and the other is not recorded; gradient factors are kept only as a pair, so this one is left out."),
                NoteKind::Dropped,
                "gradient_factor_unpaired",
                Some(member),
            ),
            _ => {}
        }
        Some(out)
    }

    /// One gradient factor as whole percent, at the scale its generator writes.
    fn gradient_factor(&mut self, value: Option<f64>) -> Option<i64> {
        let value = finite(value)?;
        if !self.percent_gradient_factors {
            return rounded(value * 100.0);
        }
        if !self.reported_gradient_scale {
            self.note(
                "$",
                PERCENT_GRADIENT_NOTE,
                NoteKind::Resolved,
                "gradient_factors_read_as_percent",
                None,
            );
            self.reported_gradient_scale = true;
        }
        rounded(value)
    }

    // -- dives -------------------------------------------------------------------------

    fn read_dives(&mut self) -> Vec<Dive> {
        let source = self.source;
        let mut dives = Vec::new();
        for (index, element) in source.dives().enumerate() {
            let path = format!("dives/{}", dives.len());
            let since = self.notes.len();
            if let Some(dive) = self.read_dive(element, index, &path) {
                dives.push(dive);
            } else {
                self.unplaced(since, &path);
            }
        }
        dives
    }

    /// Clears the member of every note since `since` about a record that was not carried.
    fn unplaced(&mut self, since: usize, record: &str) {
        let prefix = format!("{record}/");
        for note in self.notes.iter_mut().skip(since) {
            if note
                .member
                .as_deref()
                .is_some_and(|m| m == record || m.starts_with(&prefix))
            {
                note.member = None;
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one dive's members, read down the mapping document's table"
    )]
    fn read_dive(&mut self, element: &'a u::Dive, index: usize, path: &str) -> Option<Dive> {
        let at = format!("dive/{index}");
        let primary = format!("{path}/recordings/0");
        let before = &element.information_before_dive;
        let after = &element.information_after_dive;

        let started_at = self.read_started_at(before, &at, path)?;
        let uuid = self.uuid_for("dive", element.id.as_deref(), &at, &index.to_string())?;
        let mut dive = Dive::new(uuid, started_at);
        dive.number = before.dive_number.map(i64::from);

        if let Some(seconds) = finite(after.dive_duration).and_then(rounded) {
            if seconds > 0 {
                dive.submerged_time = u64::try_from(seconds).ok();
            } else {
                self.note(
                    &at,
                    format!("<diveduration> is {seconds} seconds, which is no dive time; read as not recorded."),
                    NoteKind::Absent,
                    "dive_time_zero",
                    Some(format!("{path}/submerged_time")),
                );
            }
        }
        dive.notes = notes_text(&after.notes);

        if let Some(apparatus) = &before.apparatus {
            let value = match apparatus {
                u::Apparatus::OpenScuba => Some(DiveType::OpenCircuit),
                u::Apparatus::Rebreather => Some(DiveType::ClosedCircuit),
                u::Apparatus::SurfaceSupplied => Some(DiveType::SurfaceSupplied),
                _ => None,
            };
            dive.dive_type = self.enumerated(
                value,
                ("apparatus", apparatus.as_str()),
                "dive type",
                &at,
                &format!("{path}/type"),
            );
            if matches!(apparatus, u::Apparatus::Rebreather) {
                self.note(
                    &at,
                    "UDDF's <apparatus> 'rebreather' names closed and semi-closed circuits alike; read as closed circuit.",
                    NoteKind::Resolved,
                    "rebreather_read_as_closed_circuit",
                    Some(format!("{path}/type")),
                );
            }
        }
        dive.rating = self.rating(after, &at, path);

        let max_depth = self.positive(
            after.greatest_depth,
            &at,
            "<greatestdepth>",
            "max_depth",
            path,
        );
        let mut avg_depth = self.positive(
            after.average_depth,
            &at,
            "<averagedepth>",
            "avg_depth",
            path,
        );
        if let (Some(max), Some(avg)) = (max_depth, avg_depth)
            && avg > max
        {
            self.note(
                &at,
                format!("The average depth {avg} m is deeper than the maximum depth {max} m, which cannot be; the average is left out rather than either being adjusted to fit."),
                NoteKind::Dropped,
                "average_below_maximum",
                Some(format!("{path}/avg_depth")),
            );
            avg_depth = None;
        }
        dive.max_depth = max_depth;
        dive.avg_depth = avg_depth;
        dive.bottom_temperature =
            finite(after.lowest_temperature).map(|kelvin| tidy(kelvin - KELVIN_OFFSET));
        if let Some(visibility) = finite(after.visibility) {
            if visibility >= 0.0 {
                dive.visibility = Some(visibility);
            } else {
                self.note(
                    &at,
                    format!("<visibility> is {visibility} m, below zero; it is left out."),
                    NoteKind::Dropped,
                    "visibility_negative",
                    Some(format!("{path}/visibility")),
                );
            }
        }
        let used = before.equipment_used.as_ref();
        if let Some(weight) = finite(used.and_then(|used| used.lead_quantity)) {
            if weight >= 0.0 {
                dive.weight = Some(weight);
            } else {
                self.note(
                    &at,
                    format!("<leadquantity> is {weight} kg, below zero; it is left out."),
                    NoteKind::Dropped,
                    "weight_negative",
                    Some(format!("{path}/weight")),
                );
            }
        }
        dive.altitude = self.altitude(before.altitude, &at, format!("{path}/altitude"));
        dive.air_temperature =
            finite(before.air_temperature).map(|kelvin| tidy(kelvin - KELVIN_OFFSET));
        if let Some(current) = &after.current {
            let value = match current {
                u::Current::NoCurrent => Some(Current::None),
                u::Current::VeryMildCurrent | u::Current::MildCurrent => Some(Current::Light),
                u::Current::ModerateCurrent => Some(Current::Moderate),
                u::Current::HardCurrent => Some(Current::Strong),
                u::Current::VeryHardCurrent => Some(Current::Extreme),
                _ => None,
            };
            dive.current = self.enumerated(
                value,
                ("current", current.as_str()),
                "current",
                &at,
                &format!("{path}/current"),
            );
            if matches!(current, u::Current::VeryMildCurrent) {
                self.note(
                    &at,
                    "UDDF's <current> has six steps and the logbook five; 'very-mild-current' is read as light, beside 'mild-current'.",
                    NoteKind::Resolved,
                    "current_read_as_light",
                    Some(format!("{path}/current")),
                );
            }
        }
        if let Some(platform) = &before.platform {
            let value = match platform {
                u::Platform::BeachShore | u::Platform::Landside => Some(EntryType::Shore),
                u::Platform::Pier => Some(EntryType::Pier),
                u::Platform::SmallBoat
                | u::Platform::CharterBoat
                | u::Platform::LiveAboard
                | u::Platform::Barge => Some(EntryType::Boat),
                _ => None,
            };
            dive.entry_type = self.enumerated(
                value,
                ("platform", platform.as_str()),
                "entry type",
                &at,
                &format!("{path}/entry_type"),
            );
        }

        let mut surface_pressure = None;
        if let Some(pascal) = finite(before.surface_pressure) {
            let bar = tidy(pascal / PASCAL_PER_BAR);
            if (0.4..=1.2).contains(&bar) {
                surface_pressure = Some(bar);
            } else {
                self.note(
                    &at,
                    format!("<surfacepressure> reads as {bar} bar, outside the 0.4 to 1.2 bar air pressure can be at a dive site; it is left out."),
                    NoteKind::Dropped,
                    "surface_pressure_out_of_range",
                    Some(format!("{primary}/surface_pressure")),
                );
            }
        }

        let trip = before.trip_membership.as_deref().map(|r| [r]);
        let trip_uuids = self.trip_uuids.clone();
        dive.trip_uuid = self
            .references(
                trip.iter().flatten().copied(),
                &trip_uuids,
                &at,
                "trip",
                &format!("{path}/trip_uuid"),
            )
            .into_iter()
            .next();

        let links: Vec<&str> = before.links.iter().map(String::as_str).collect();
        let to_contacts: Vec<&str> = links
            .iter()
            .copied()
            .filter(|r| {
                !self.site_uuids.contains_key(r)
                    && (self.contact_uuids.contains_key(r) || self.unread_contacts.contains(r))
            })
            .collect();
        let to_people: Vec<&str> = links
            .iter()
            .copied()
            .filter(|r| {
                !self.site_uuids.contains_key(r)
                    && !to_contacts.contains(r)
                    && (self.person_uuids.contains_key(r)
                        || self.unread_people.contains(r)
                        || self.guides.contains_key(r))
            })
            .collect();
        dive.contact_uuid = self.dive_contact(&to_contacts, &at, &format!("{path}/contact_uuid"));
        let to_sites = links
            .iter()
            .copied()
            .filter(|r| !to_contacts.contains(r) && !to_people.contains(r));
        let site_uuids = self.site_uuids.clone();
        dive.site_uuids = self.references(
            to_sites,
            &site_uuids,
            &at,
            "dive site",
            &format!("{path}/site_uuids"),
        );
        let gear_uuids = self.gear_uuids.clone();
        dive.gear_uuids = self.references(
            used.iter()
                .flat_map(|used| used.links.iter().map(String::as_str)),
            &gear_uuids,
            &at,
            "piece of gear",
            &format!("{path}/gear_uuids"),
        );
        dive.people = self.dive_people(&to_people, &at, &format!("{path}/people"));

        let (mut cylinders, mix_refs) = self.read_cylinders(element, &at, path);
        let (profile, needs_gas_numbers, mode) =
            self.read_profile(element, &at, &mix_refs, &primary);
        if needs_gas_numbers {
            for (number, cylinder) in cylinders.iter_mut().enumerate() {
                cylinder.gas_number = u32::try_from(number).ok();
            }
            self.note(
                &at,
                "UDDF records no gas numbers, so the dive's cylinders are numbered from 0 in the order the file lists them, to tie each tank pressure and gas switch to its cylinder.",
                NoteKind::Absent,
                "gas_numbers_assigned",
                Some(format!("{path}/cylinders")),
            );
        }
        dive.cylinders = cylinders;
        let model = self.read_deco_model(before, &at, &primary);
        dive.recordings =
            self.read_recordings(before, profile, mode, model, surface_pressure, &at, path);
        Some(dive)
    }

    fn enumerated<T>(
        &mut self,
        value: Option<T>,
        (tag, stated): (&str, &str),
        label: &str,
        at: &str,
        member: &str,
    ) -> Option<T> {
        if value.is_none() {
            self.note(
                at,
                format!("<{tag}> is {stated:?}, which the logbook's {label} has no value for; it is left out."),
                NoteKind::Dropped,
                "value_unknown",
                Some(member.to_owned()),
            );
        }
        value
    }

    fn rating(&mut self, after: &u::InformationAfterDive, at: &str, path: &str) -> Option<u8> {
        let steps = after.rating.as_ref()?.value?;
        if !(1..=10).contains(&steps) {
            self.note(
                at,
                format!("<ratingvalue> is {steps}, and UDDF rates a dive in whole steps from 1 to 10; it is left out."),
                NoteKind::Dropped,
                "rating_out_of_range",
                Some(format!("{path}/rating")),
            );
            return None;
        }
        let rating = steps.div_ceil(2);
        self.note(
            at,
            format!("UDDF rates a dive 1 to 10 and the logbook 1 to 5; <ratingvalue> {steps} is read as {rating}, half of it rounded up."),
            NoteKind::Resolved,
            "rating_halved",
            Some(format!("{path}/rating")),
        );
        Some(rating)
    }

    fn positive(
        &mut self,
        value: Option<f64>,
        at: &str,
        source: &str,
        member: &str,
        path: &str,
    ) -> Option<f64> {
        let value = finite(value)?;
        if value > 0.0 {
            return Some(value);
        }
        self.note(
            at,
            format!("{source} is {value}, which is no depth; read as not recorded."),
            NoteKind::Absent,
            "depth_zero",
            Some(format!("{path}/{member}")),
        );
        None
    }

    fn read_started_at(
        &mut self,
        before: &u::InformationBeforeDive,
        at: &str,
        path: &str,
    ) -> Option<StartedAt> {
        let member = Some(format!("{path}/started_at"));
        let Some(raw) = before.datetime else {
            self.note(
                at,
                "The dive records no <datetime>, and a dive cannot be logged without a start time; it is left out.",
                NoteKind::Dropped,
                "start_time_missing",
                None,
            );
            return None;
        };
        if raw.time.is_none() {
            let date = Date::try_from(raw.date).ok()?;
            self.note(
                at,
                format!("{:?} records a date with no time of day, so the dive starts on that day rather than at midnight.", date_text(raw.date)),
                NoteKind::Absent,
                "start_time_of_day_missing",
                member.clone(),
            );
            if raw.offset.is_some() {
                self.note(
                    at,
                    "The start records a time zone beside a date alone, which a date cannot carry; it is left out.",
                    NoteKind::Dropped,
                    "start_time_zone_on_a_date",
                    member,
                );
            }
            return Some(StartedAt::Date(date));
        }
        let mut value = raw;
        if self.local_clock_with_z && value.offset == Some(u::Offset::Utc) {
            self.note(
                at,
                LOCAL_CLOCK_NOTE,
                NoteKind::Resolved,
                "start_time_z_read_as_local",
                member,
            );
            value.offset = None;
        } else if value.offset.is_none() {
            self.note(
                at,
                "The file gives the start time with no time zone, so it is kept as a local time.",
                NoteKind::Absent,
                "start_time_zone_missing",
                member,
            );
        }
        if let Ok(started) = value.to_string().parse::<DateTime>() {
            return Some(StartedAt::DateTime(started));
        }
        self.note(
            at,
            format!("<datetime> is {raw:?}, which is not a date and time, and a dive cannot be logged without a start time; it is left out."),
            NoteKind::Dropped,
            "start_time_unreadable",
            None,
        );
        None
    }

    fn dive_contact(&mut self, refs: &[&str], at: &str, member: &str) -> Option<Uuid> {
        let mut found: Vec<Uuid> = Vec::new();
        for reference in refs {
            if let Some(uuid) = self.contact_uuids.get(reference).copied() {
                if !found.contains(&uuid) {
                    found.push(uuid);
                }
            } else {
                self.note(
                    at,
                    format!("A link points at the contact {reference:?}, which is not kept; the link goes with it."),
                    NoteKind::Dropped,
                    "link_to_record_not_kept",
                    Some(member.to_owned()),
                );
            }
        }
        if found.len() > 1 {
            self.note(
                at,
                format!("The dive links {} contacts and keeps one; the first is kept and the rest are left out.", found.len()),
                NoteKind::Dropped,
                "contacts_beyond_first",
                Some(member.to_owned()),
            );
        }
        found.first().copied()
    }

    fn dive_people(&mut self, refs: &[&str], at: &str, member: &str) -> Vec<PersonReference> {
        let mut people: Vec<PersonReference> = Vec::new();
        let mut repeated: Vec<(Uuid, &str, u32)> = Vec::new();
        for &reference in refs {
            let (buddy, role) = if let Some(&guide) = self.guides.get(reference) {
                if let Some(buddy) = guide.filter(|buddy| self.person_uuids.contains_key(buddy)) {
                    (buddy, PersonRole::Guide)
                } else {
                    {
                        self.note(
                            at,
                            format!("A link points at the guide {reference:?}, whose own link names no buddy that is kept; the link goes with it."),
                            NoteKind::Dropped,
                            "link_to_record_not_kept",
                            Some(member.to_owned()),
                        );
                        continue;
                    }
                }
            } else if self.person_uuids.contains_key(reference) {
                let role = if self.students.contains(reference) {
                    PersonRole::Student
                } else {
                    PersonRole::Buddy
                };
                (reference, role)
            } else {
                self.note(
                    at,
                    format!("A link points at the buddy {reference:?}, which is not kept; the link goes with it."),
                    NoteKind::Dropped,
                    "link_to_record_not_kept",
                    Some(member.to_owned()),
                );
                continue;
            };
            let Some(uuid) = self.person_uuids.get(buddy).copied() else {
                continue;
            };
            if let Some(held) = people.iter_mut().find(|p| p.person_uuid == uuid) {
                if role == PersonRole::Guide {
                    held.role = Some(PersonRole::Guide);
                }
                if let Some(entry) = repeated.iter_mut().find(|(u, _, _)| *u == uuid) {
                    entry.2 = entry.2.saturating_add(1);
                } else {
                    repeated.push((uuid, buddy, 2));
                }
                continue;
            }
            people.push(PersonReference::new(uuid, Some(role)));
        }
        for (uuid, buddy, count) in repeated {
            let role = match people
                .iter()
                .find(|p| p.person_uuid == uuid)
                .and_then(|p| p.role.as_ref())
            {
                Some(PersonRole::Guide) => "guide",
                Some(PersonRole::Student) => "student",
                _ => "buddy",
            };
            self.note(
                at,
                format!("The dive links the buddy {buddy:?} {count} times, and a person is listed on a dive once; one link is kept, as {role}, and the rest are left out."),
                NoteKind::Dropped,
                "person_named_twice",
                Some(member.to_owned()),
            );
        }
        people
    }

    /// Resolved references, in source order, without repeats.
    fn references<'r>(
        &mut self,
        refs: impl Iterator<Item = &'r str>,
        table: &HashMap<&str, Uuid>,
        at: &str,
        kind: &str,
        member: &str,
    ) -> Vec<Uuid> {
        let mut resolved = Vec::new();
        for reference in refs {
            if let Some(uuid) = table.get(reference) {
                if !resolved.contains(uuid) {
                    resolved.push(*uuid);
                }
            } else if !self.defined(reference) {
                self.link_to_nothing(at, reference, Some(member.to_owned()));
            } else if !self.mixes.contains_key(reference)
                && !self.deco_models.contains_key(reference)
            {
                self.note(
                    at,
                    format!("A link points at {reference:?}, which is not a {kind} that is kept; the link is left out."),
                    NoteKind::Dropped,
                    "link_to_unknown_kind",
                    Some(member.to_owned()),
                );
            }
        }
        resolved
    }

    fn link_to_nothing(&mut self, at: &str, reference: &str, member: Option<String>) {
        self.note(
            at,
            format!("A link points at {reference:?}, which nothing in the file defines; the link is left out."),
            NoteKind::Dropped,
            "link_to_nothing",
            member,
        );
    }

    // -- cylinders ---------------------------------------------------------------------

    fn read_cylinders(
        &mut self,
        element: &'a u::Dive,
        at: &str,
        dive: &str,
    ) -> (Vec<Cylinder>, Vec<Option<&'a str>>) {
        let mut cylinders = Vec::new();
        let mut mix_refs = Vec::new();
        let mut vessels: HashMap<Uuid, usize> = HashMap::new();
        for (index, tank) in element.tank_data.iter().enumerate() {
            let tank_at = format!("{at}/tankdata/{index}");
            let path = format!("{dive}/cylinders/{index}");
            let mut cylinder = Cylinder::default();
            let (mix_ref, vessel_ref, gear_uuid) =
                self.tankdata_links(tank, &tank_at, index, &mut vessels, &path);

            match finite(tank.tank_volume) {
                None => {
                    if let Some(volume) = vessel_ref.and_then(|r| self.tank_volumes.get(r)) {
                        cylinder.volume = Some(*volume);
                    } else {
                        self.note(
                            &tank_at,
                            "The file records no cylinder size, so it is left empty.",
                            NoteKind::Absent,
                            "cylinder_size_missing",
                            Some(format!("{path}/volume")),
                        );
                    }
                }
                Some(raw) => {
                    let (litres, reinterpreted) = volume_litres(raw);
                    if reinterpreted {
                        self.size_as_litres(&tank_at, raw, litres, Some(format!("{path}/volume")));
                    }
                    if litres > 0.0 {
                        cylinder.volume = Some(litres);
                    } else {
                        self.size_zero(&tank_at, litres, Some(format!("{path}/volume")));
                    }
                }
            }

            let mut start = self.pressure_bar(
                tank.tank_pressure_begin,
                &tank_at,
                "<tankpressurebegin>",
                &format!("{path}/start_pressure"),
            );
            let mut end = self.pressure_bar(
                tank.tank_pressure_end,
                &tank_at,
                "<tankpressureend>",
                &format!("{path}/end_pressure"),
            );
            if start.is_some_and(|start| start <= 0.0) {
                self.note(
                    &tank_at,
                    "The start pressure is 0 bar, which dive computers write to mean 'not recorded'; read as not recorded.",
                    NoteKind::Absent,
                    "start_pressure_zero",
                    Some(format!("{path}/start_pressure")),
                );
                start = None;
            }
            if let (Some(s), Some(e)) = (start, end)
                && e > s
            {
                self.note(
                    &tank_at,
                    format!("The end pressure {e} bar is above the start pressure {s} bar, which cannot be; the end pressure is left out."),
                    NoteKind::Dropped,
                    "end_pressure_above_start",
                    Some(format!("{path}/end_pressure")),
                );
                end = None;
            }
            cylinder.start_pressure = start;
            cylinder.end_pressure = end;

            match mix_ref {
                None => self.note(
                    &tank_at,
                    "No gas is recorded for this cylinder, so it is left empty rather than assumed to be air.",
                    NoteKind::Absent,
                    "gas_missing",
                    Some(format!("{path}/oxygen")),
                ),
                Some(reference) => {
                    if let Some(gas) = self.mixes.get(reference).copied() {
                        cylinder.oxygen = gas.oxygen;
                        cylinder.helium = gas.helium;
                        cylinder.ppo2_limit = gas.ppo2_limit;
                    } else {
                        self.note(
                            &tank_at,
                            format!("The cylinder links the gas {reference:?}, which <gasdefinitions> does not define, so its gas is left empty."),
                            NoteKind::Absent,
                            "gas_link_to_nothing",
                            Some(format!("{path}/oxygen")),
                        );
                    }
                }
            }
            cylinder.gear_uuid = gear_uuid;
            cylinders.push(cylinder);
            mix_refs.push(mix_ref);
        }
        (cylinders, mix_refs)
    }

    fn tankdata_links(
        &mut self,
        tank: &'a u::TankData,
        at: &str,
        index: usize,
        vessels: &mut HashMap<Uuid, usize>,
        path: &str,
    ) -> (Option<&'a str>, Option<&'a str>, Option<Uuid>) {
        let mut mix_ref: Option<&'a str> = None;
        let mut vessel_ref: Option<&'a str> = None;
        let mut kept = None;
        let mut unresolved = Vec::new();
        for reference in &tank.links {
            let reference = reference.as_str();
            if self.mixes.contains_key(reference) {
                if let Some(first) = mix_ref {
                    self.note(
                        at,
                        format!("The cylinder links a second gas, {reference:?}, and holds one; the first, {first:?}, is kept."),
                        NoteKind::Dropped,
                        "second_gas_link",
                        Some(path.to_owned()),
                    );
                } else {
                    mix_ref = Some(reference);
                }
            } else if let Some(gear_uuid) = self.gear_uuids.get(reference).copied() {
                if let Some(first) = vessel_ref {
                    self.note(
                        at,
                        format!("The cylinder links a second tank, {reference:?}, and is one; the first, {first:?}, is kept."),
                        NoteKind::Dropped,
                        "second_tank_link",
                        Some(format!("{path}/gear_uuid")),
                    );
                    continue;
                }
                vessel_ref = Some(reference);
                if let Some(other) = vessels.get(&gear_uuid) {
                    self.note(
                        at,
                        format!("The cylinder links the tank {reference:?}, which <tankdata> {other} of this dive already links, and one tank is one gas supply; the cylinder keeps its gas and the link is left out."),
                        NoteKind::Dropped,
                        "tank_linked_twice",
                        Some(format!("{path}/gear_uuid")),
                    );
                } else {
                    vessels.insert(gear_uuid, index);
                    kept = Some(gear_uuid);
                }
            } else {
                unresolved.push(reference);
            }
        }
        let mut unresolved = unresolved.into_iter();
        if mix_ref.is_none() {
            mix_ref = unresolved.next();
        }
        for reference in unresolved {
            if self.defined(reference) {
                self.note(
                    at,
                    format!("A link points at {reference:?}, which is neither a gas nor a piece of gear that is kept; the link is left out."),
                    NoteKind::Dropped,
                    "link_to_unknown_kind",
                    Some(path.to_owned()),
                );
            } else {
                self.link_to_nothing(at, reference, Some(path.to_owned()));
            }
        }
        (mix_ref, vessel_ref, kept)
    }

    fn pressure_bar(
        &mut self,
        pascal: Option<f64>,
        at: &str,
        label: &str,
        member: &str,
    ) -> Option<f64> {
        let bar = tidy(finite(pascal)? / PASCAL_PER_BAR);
        if (0.0..=350.0).contains(&bar) {
            return Some(bar);
        }
        self.note(
            at,
            format!("{label} reads as {bar} bar, outside the 0 to 350 bar a cylinder can hold; it is left out."),
            NoteKind::Dropped,
            "pressure_out_of_range",
            Some(member.to_owned()),
        );
        None
    }

    // -- profile -----------------------------------------------------------------------

    #[allow(
        clippy::too_many_lines,
        reason = "every channel of a waypoint, in the mapping document's order"
    )]
    fn read_profile(
        &mut self,
        element: &'a u::Dive,
        at: &str,
        mix_refs: &[Option<&str>],
        recording: &str,
    ) -> (Option<Profile>, bool, Option<RecordingMode>) {
        if element.samples.is_empty() {
            return (None, false, None);
        }
        let profile_path = format!("{recording}/profile");
        let mut offered: Vec<(i64, &u::Waypoint)> = Vec::new();
        for (index, waypoint) in element.samples.iter().enumerate() {
            let here = format!("{at}/waypoint/{index}");
            match finite(waypoint.dive_time).and_then(milliseconds) {
                None => self.note(
                    &here,
                    "A waypoint records no <divetime>, so it has no place in the dive's profile and is left out.",
                    NoteKind::Dropped,
                    "sample_time_missing",
                    Some(profile_path.clone()),
                ),
                Some(ms) if ms < 0 => self.note(
                    &here,
                    format!("A waypoint is at {} s, before the dive began, and is left out.", in_seconds(ms)),
                    NoteKind::Dropped,
                    "sample_before_start",
                    Some(profile_path.clone()),
                ),
                Some(ms) => offered.push((ms, waypoint)),
            }
        }
        offered.sort_by_key(|(ms, _)| *ms);
        let mut ordered: Vec<(u64, &u::Waypoint)> = Vec::new();
        for (ms, waypoint) in offered {
            let Ok(ms) = u64::try_from(ms) else { continue };
            if ordered.last().is_some_and(|(last, _)| *last == ms) {
                self.note(
                    at,
                    format!("Two waypoints are both at {} s, to the millisecond; the later one is left out, a profile holding one reading per instant.", in_seconds(i64::try_from(ms).unwrap_or(0))),
                    NoteKind::Dropped,
                    "readings_at_one_instant",
                    Some(profile_path.clone()),
                );
                continue;
            }
            ordered.push((ms, waypoint));
        }

        let mut cylinders_of_mix: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, reference) in mix_refs.iter().enumerate() {
            if let Some(reference) = reference {
                cylinders_of_mix.entry(reference).or_default().push(index);
            }
        }

        let mut depth = Channel::new("depth", None);
        let mut temperature = Channel::new("temperature", None);
        let mut ndl = Channel::new("ndl", Some(0));
        let mut ppo2 = Channel::new("ppo2", Some(0));
        let mut cns = Channel::new("cns", Some(0));
        let mut gradient_factor = Channel::new("gradient_factor", Some(0));
        let mut pressures: Vec<(usize, Channel)> = Vec::new();
        let mut events: Vec<Event> = Vec::new();
        let mut needs_gas_numbers = false;
        let mut mode = None;
        let mut reported_pascal_po2 = false;

        for &(ms, waypoint) in &ordered {
            if let Some(m) = finite(waypoint.depth).and_then(|m| rounded(m * 100.0)) {
                depth.record(ms, m);
            }
            if let Some(t) =
                finite(waypoint.temperature).and_then(|k| rounded((k - KELVIN_OFFSET) * 100.0))
            {
                temperature.record(ms, t);
            }
            if let Some(s) = finite(waypoint.no_deco_time).and_then(rounded) {
                ndl.record(ms, s);
            }
            if let Some(value) = finite(waypoint.calculated_po2) {
                let (hundredths, pascal) = if value <= 10.0 {
                    (rounded(value * 100.0), false)
                } else {
                    (rounded(value / 1000.0), true)
                };
                if pascal && !reported_pascal_po2 {
                    self.note(
                        at,
                        "<calculatedpo2> is above 10, so it is read as the Pascal the UDDF documentation states rather than the bar most files give.",
                        NoteKind::Resolved,
                        "ppo2_read_as_pascal",
                        Some(format!("{profile_path}/ppo2")),
                    );
                    reported_pascal_po2 = true;
                }
                if let Some(hundredths) = hundredths {
                    ppo2.record(ms, hundredths);
                }
            }
            if let Some(tenths) = finite(waypoint.cns).and_then(|p| rounded(p * 10.0)) {
                cns.record(ms, tenths);
            }
            let factor = waypoint.gradient_factor.as_ref().map(|f| f.value);
            if let Some(factor) = self.gradient_factor(factor) {
                gradient_factor.record(ms, factor);
            }
            mode = self.waypoint_mode(waypoint, mode, ms, at, &format!("{recording}/mode"));

            for (index, tenths) in self.waypoint_pressures(
                waypoint,
                at,
                ms,
                &cylinders_of_mix,
                mix_refs.len(),
                &format!("{profile_path}/pressures"),
            ) {
                let channel =
                    if let Some((_, channel)) = pressures.iter_mut().find(|(i, _)| *i == index) {
                        channel
                    } else {
                        pressures.push((index, Channel::new("pressures", None)));
                        match pressures.last_mut() {
                            Some((_, channel)) => channel,
                            None => continue,
                        }
                    };
                if channel.taken(ms) {
                    self.note(
                        at,
                        format!("Two tank pressures at {} s belong to the same cylinder; the later one is left out.", in_seconds(i64::try_from(ms).unwrap_or(0))),
                        NoteKind::Dropped,
                        "readings_at_one_instant",
                        Some(format!("{profile_path}/pressures")),
                    );
                    continue;
                }
                channel.record(ms, tenths);
                needs_gas_numbers = true;
            }

            if let Some(marker) = text(waypoint.set_marker.as_ref()) {
                let kind = match marker {
                    "deep_stop" => Some(EventType::DeepStop),
                    "safety_stop" => Some(EventType::SafetyStop),
                    "bookmark" => Some(EventType::Bookmark),
                    _ => None,
                };
                let mut event = Event::new(ms, kind.unwrap_or(EventType::Bookmark));
                if kind.is_none() {
                    // No `type` at all: §6.6's spelling of an unclassified event.
                    event.event_type = None;
                    event.label = Some(marker.to_owned());
                }
                events.push(event);
            }
            if let Some(reference) = waypoint.switch_mix.as_deref() {
                let mut event = Event::new(ms, EventType::GasSwitch);
                if let Some(index) = cylinders_of_mix.get(reference).and_then(|c| c.first()) {
                    event.gas_number = u32::try_from(*index).ok();
                    needs_gas_numbers = true;
                } else {
                    self.note(
                        at,
                        format!("A gas switch at {} s names the gas {reference:?}, which no cylinder on this dive links to; the switch is kept without saying which cylinder it was to.", in_seconds(i64::try_from(ms).unwrap_or(0))),
                        NoteKind::Absent,
                        "gas_switch_target_unknown",
                        Some(format!("{profile_path}/events")),
                    );
                }
                events.push(event);
            }
        }

        for channel in [&depth, &temperature, &ndl, &ppo2, &cns, &gradient_factor] {
            if channel.refused > 0 {
                let one = channel.refused == 1;
                self.note(
                    at,
                    format!(
                        "{} of the dive's {} readings {} below zero, which is how a device writes a reading it does not have; {} left out.",
                        channel.refused,
                        channel.name,
                        if one { "is" } else { "are" },
                        if one { "that reading is" } else { "those readings are" }
                    ),
                    NoteKind::Dropped,
                    "negative_readings",
                    Some(format!("{profile_path}/{}", channel.name)),
                );
            }
        }
        if ordered.is_empty() {
            return (None, false, mode);
        }
        pressures.sort_by_key(|(index, _)| *index);
        let singles = [&depth, &temperature, &ndl, &ppo2, &cns, &gradient_factor];
        if singles.iter().all(|c| c.times.is_empty()) && pressures.is_empty() && events.is_empty() {
            let count = ordered.len();
            let subject = if count == 1 {
                "waypoint carries"
            } else {
                "waypoints carry"
            };
            self.note(
                at,
                format!("The dive's {count} {subject} a time and no reading that can be kept, so the dive has no profile."),
                NoteKind::Dropped,
                "profile_empty",
                Some(profile_path),
            );
            return (None, false, mode);
        }
        let duration = singles
            .iter()
            .map(|c| &c.times)
            .chain(pressures.iter().map(|(_, c)| &c.times))
            .filter_map(|times| times.last().copied())
            .max()
            .unwrap_or(0);
        let mut profile = Profile::new(duration);
        profile.depth = depth.into_series();
        profile.temperature = temperature.into_series();
        profile.ndl = ndl.into_series();
        profile.ppo2 = ppo2.into_series();
        profile.cns = cns.into_series();
        profile.gradient_factor = gradient_factor.into_series();
        profile.pressures = pressures
            .into_iter()
            .filter_map(|(index, channel)| {
                Some(PressureSeries::new(
                    channel.times,
                    channel.values,
                    u32::try_from(index).ok()?,
                ))
            })
            .collect();
        events.sort_by_key(|event| event.time);
        profile.events = events;
        (Some(profile), needs_gas_numbers, mode)
    }

    fn waypoint_mode(
        &mut self,
        waypoint: &u::Waypoint,
        standing: Option<RecordingMode>,
        ms: u64,
        at: &str,
        member: &str,
    ) -> Option<RecordingMode> {
        let Some(stated) = &waypoint.dive_mode else {
            return standing;
        };
        let mode = match stated {
            u::DiveMode::OpenCircuit => Some(RecordingMode::OpenCircuit),
            u::DiveMode::ClosedCircuit => Some(RecordingMode::ClosedCircuit),
            u::DiveMode::SemiClosedCircuit => Some(RecordingMode::SemiClosed),
            u::DiveMode::Apnea | u::DiveMode::Apnoe => Some(RecordingMode::Freedive),
            _ => None,
        };
        let Some(mode) = mode else {
            let said = if matches!(stated, u::DiveMode::Unspecified) {
                "no dive mode".to_owned()
            } else {
                format!("the dive mode {:?}", stated.as_str())
            };
            self.note(
                at,
                format!("A waypoint records {said}, which is not one of the five UDDF names; the dive computer's mode is left empty rather than assumed."),
                NoteKind::Dropped,
                "dive_mode_unknown",
                Some(member.to_owned()),
            );
            return standing;
        };
        let Some(standing) = standing else {
            return Some(mode);
        };
        if mode != standing {
            self.note(
                at,
                format!("The dive mode changes to {:?} at {} s, and one dive computer's record keeps one mode; the change is left out.", stated.as_str(), in_seconds(i64::try_from(ms).unwrap_or(0))),
                NoteKind::Dropped,
                "dive_mode_change",
                Some(member.to_owned()),
            );
        }
        Some(standing)
    }

    fn waypoint_pressures(
        &mut self,
        waypoint: &u::Waypoint,
        at: &str,
        ms: u64,
        cylinders_of_mix: &HashMap<&str, Vec<usize>>,
        tank_count: usize,
        member: &str,
    ) -> Vec<(usize, i64)> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut out = Vec::new();
        let when = in_seconds(i64::try_from(ms).unwrap_or(0));
        for pressure in &waypoint.tank_pressures {
            let Some(pascal) = finite(Some(pressure.value)) else {
                continue;
            };
            let index = if let Some(reference) = pressure.tank_ref.as_deref() {
                let position = seen.entry(reference).or_insert(0);
                let candidate = cylinders_of_mix
                    .get(reference)
                    .and_then(|c| c.get(*position))
                    .copied();
                *position = position.saturating_add(1);
                let Some(index) = candidate else {
                    self.note(
                        at,
                        format!("A tank pressure at {when} s names the gas {reference:?}, which no further cylinder on this dive links to; the reading is left out."),
                        NoteKind::Dropped,
                        "tank_pressure_unassigned",
                        Some(member.to_owned()),
                    );
                    continue;
                };
                index
            } else if tank_count == 1 {
                0
            } else {
                self.note(
                    at,
                    format!("A tank pressure at {when} s names no cylinder, and the dive has {tank_count}; the reading is left out rather than guessed onto one."),
                    NoteKind::Dropped,
                    "tank_pressure_unassigned",
                    Some(member.to_owned()),
                );
                continue;
            };
            if let Some(tenths) = rounded(pascal / PASCAL_PER_BAR * 10.0) {
                out.push((index, tenths));
            }
        }
        out
    }

    // -- recordings --------------------------------------------------------------------

    #[allow(
        clippy::too_many_arguments,
        reason = "what a dive states once, handed to its recordings"
    )]
    fn read_recordings(
        &mut self,
        before: &u::InformationBeforeDive,
        profile: Option<Profile>,
        mode: Option<RecordingMode>,
        model: Option<DecoModel>,
        surface_pressure: Option<f64>,
        at: &str,
        path: &str,
    ) -> Vec<Recording> {
        let counter = before.internal_dive_number;
        let linked: Vec<&u::EquipmentPiece> = before
            .equipment_used
            .iter()
            .flat_map(|used| used.links.iter())
            .filter_map(|r| self.computers.get(r.as_str()).copied())
            .collect();
        let build = |device: Option<Device>,
                     mode: Option<RecordingMode>,
                     model: Option<DecoModel>,
                     surface_pressure: Option<f64>,
                     profile: Option<Profile>| {
            if device.is_none() && profile.is_none() && surface_pressure.is_none() {
                return None;
            }
            Some(Recording {
                device,
                mode,
                deco_model: model,
                surface_pressure,
                profile,
                ..Recording::default()
            })
        };
        if linked.is_empty() {
            if counter.is_some() {
                self.note(
                    at,
                    "The dive records the dive computer's own dive count and links no <divecomputer> for it to belong to; it is left out.",
                    NoteKind::Dropped,
                    "dive_counter_without_computer",
                    None,
                );
            }
            return build(None, mode, model, surface_pressure, profile)
                .into_iter()
                .collect();
        }
        let mut recordings = Vec::new();
        let mut firsts = Some((counter, mode, model, surface_pressure, profile));
        for element in linked {
            let slot = format!("{path}/recordings/{}", recordings.len());
            let (counter, mode, model, surface_pressure, profile) =
                firsts.take().unwrap_or_default();
            let since = self.notes.len();
            let device = self.read_device(element, counter, at, &slot);
            if let Some(recording) = build(device, mode, model, surface_pressure, profile) {
                recordings.push(recording);
            } else {
                self.unplaced(since, &slot);
            }
        }
        if recordings.len() > 1 && surface_pressure.is_some() {
            self.note(
                at,
                format!("The dive states <surfacepressure> once and was recorded by {} dive computers; it is read as the first computer's figure, the file not saying which computer worked it out.", recordings.len()),
                NoteKind::Resolved,
                "figure_given_to_first_computer",
                Some(format!("{path}/recordings/0/surface_pressure")),
            );
        }
        recordings
    }

    fn read_device(
        &mut self,
        element: &u::EquipmentPiece,
        counter: Option<u32>,
        at: &str,
        path: &str,
    ) -> Option<Device> {
        let mut device = Device::default();
        let mut string = |value: Option<&String>, label: &str| {
            text(value).map(|v| self.capped(v, MAX_DEVICE, at, label))
        };
        device.brand = string(
            element.manufacturer.as_ref().and_then(|m| m.name.as_ref()),
            "<manufacturer><name>",
        );
        device.model = string(element.model.as_ref(), "<model>");
        device.serial = string(element.serial_number.as_ref(), "<serialnumber>");
        device.name = string(element.name.as_ref(), "<name>");
        device.dive_number = counter.map(u64::from);
        let _ = path;
        (device != Device::default()).then_some(device)
    }
}

/// Fills a standing contact's member from an inline shape, or records that they differ.
fn fill<T: PartialEq>(
    standing: &mut Option<T>,
    value: Option<T>,
    member: &'static str,
    differing: &mut Vec<&'static str>,
) {
    let Some(value) = value else { return };
    match standing {
        None => *standing = Some(value),
        Some(held) if *held != value => differing.push(member),
        Some(_) => {}
    }
}

/// One sampled quantity on its own axis.
struct Channel {
    name: &'static str,
    floor: Option<i64>,
    times: Vec<u64>,
    values: Vec<i64>,
    refused: usize,
}

impl Channel {
    const fn new(name: &'static str, floor: Option<i64>) -> Self {
        Self {
            name,
            floor,
            times: Vec::new(),
            values: Vec::new(),
            refused: 0,
        }
    }

    fn taken(&self, at: u64) -> bool {
        self.times.last() == Some(&at)
    }

    fn record(&mut self, at: u64, value: i64) {
        if self.floor.is_some_and(|floor| value < floor) {
            self.refused = self.refused.saturating_add(1);
            return;
        }
        if self.taken(at) {
            return;
        }
        self.times.push(at);
        self.values.push(value);
    }

    fn into_series(self) -> Option<Series> {
        (!self.times.is_empty()).then(|| Series::new(self.times, self.values))
    }
}
