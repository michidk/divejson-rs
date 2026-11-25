//! DiveJSON out as UDDF, following `docs/uddf-writing.md` and `docs/writing.md`.

use std::collections::{BTreeMap, HashMap, HashSet};

use ::uddf as u;
use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::Serialize;

use super::{
    KELVIN_OFFSET, LITRES_PER_CUBIC_METRE, Note, NoteKind, PASCAL_PER_BAR, Report, in_seconds, tidy,
};
use crate::{
    Contact, ContactRole, Current, Cylinder, Device, Dive, DiveType, Document, EntryType, Error,
    EventType, GearItem, GearType, Material, Person, PersonReference, PersonRole, Profile,
    Recording, RecordingMode, Series, Uuid, folded,
};

/// The `@version` this writer declares: 3.2.2 is the newest published schema, and the
/// writer emits nothing 3.2.3 added.
const DECLARED_VERSION: &str = "3.2.2";

/// Why sightings and species are not written (`docs/uddf-writing.md`, *What is never written*).
const NO_SPECIES: &str = "UDDF's <observations> holds a <species> only under a class inside <vertebrata>, <invertebrata> or <flora>, which this format does not record and a writer would have to invent, and its spelling for animals of no known class, names in a <fauna><notes> paragraph, carries no count and no note and reads back as a remark";

/// Writes a DiveJSON document as UDDF 3.2.2, with the report of what UDDF could not hold.
///
/// The output is a function of the document alone: `<generator><datetime>` is the
/// document's `exported_at`, and no clock is read. The document is expected to conform
/// (see [`Document::validate`]); a member out of range is written as it stands.
///
/// # Errors
///
/// [`Error::Uddf`] if serialising the XML fails.
pub fn write(doc: &Document) -> Result<(String, Report), Error> {
    let mut writer = Writer::new(doc);
    let model = writer.run();
    let text = u::to_string(&model)?;
    let root = format!(
        "<uddf xmlns=\"{}\" version=\"{}\">",
        u::NAMESPACE,
        u::VERSION
    );
    let declared = format!(
        "<uddf xmlns=\"{}\" version=\"{DECLARED_VERSION}\">",
        u::NAMESPACE
    );
    Ok((text.replacen(&root, &declared, 1), writer.notes))
}

fn id(prefix: &str, uuid: Uuid) -> String {
    format!("{prefix}-{uuid}")
}

fn para(text: &str) -> Vec<u::NoteItem> {
    vec![u::NoteItem::Para(text.to_owned())]
}

fn midnight(date: crate::Date) -> Option<u::DateTime> {
    format!("{date}T00:00:00").parse().ok()
}

/// One free-text name as `personalType`'s first and last name, split at the first space.
fn person_names(name: &str) -> (String, String) {
    let mut tokens = name.split_whitespace();
    let first = tokens.next().unwrap_or_default().to_owned();
    (first, tokens.collect::<Vec<_>>().join(" "))
}

fn fold(value: Option<&String>) -> Option<String> {
    value.map(|v| folded(v)).filter(|v| !v.is_empty())
}

fn agree(one: Option<&String>, other: Option<&String>) -> bool {
    match (fold(one), fold(other)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// A blend and the limit planned for it: what makes two cylinders one `<mix>`.
#[derive(Clone, Copy, PartialEq)]
struct MixKey {
    oxygen: Option<f64>,
    helium: Option<f64>,
    ppo2_limit: Option<f64>,
}

impl MixKey {
    fn of(cylinder: &Cylinder) -> Self {
        Self {
            oxygen: cylinder.oxygen,
            helium: cylinder.helium,
            ppo2_limit: cylinder.ppo2_limit,
        }
    }

    fn sort_key(self) -> [f64; 3] {
        [self.oxygen, self.helium, self.ppo2_limit].map(|v| v.unwrap_or(-1.0))
    }

    fn name(self) -> String {
        match (self.oxygen, self.helium) {
            (oxygen, Some(helium)) if helium != 0.0 => {
                format!("Trimix {}/{helium}", oxygen.unwrap_or(0.0))
            }
            (None, _) => "unrecorded".to_owned(),
            (Some(oxygen), _) if (oxygen - 21.0).abs() < f64::EPSILON => "Air".to_owned(),
            (Some(oxygen), _) if (oxygen - 100.0).abs() < f64::EPSILON => "Oxygen".to_owned(),
            (Some(oxygen), _) => format!("EAN{oxygen}"),
        }
    }
}

/// How many cylinders before this one of the same dive carry the same mix.
fn occurrence(seen: &mut Vec<(MixKey, usize)>, key: MixKey) -> usize {
    if let Some((_, count)) = seen.iter_mut().find(|(k, _)| *k == key) {
        *count = count.saturating_add(1);
        *count
    } else {
        seen.push((key, 0));
        0
    }
}

/// One computer and every recording that is its record of a dive.
struct DeviceRecord {
    merged: Device,
    recordings: Vec<(usize, usize)>,
    gear_index: Option<usize>,
    element_id: Option<String>,
    unfolded: Vec<(usize, usize)>,
}

impl DeviceRecord {
    fn label(&self) -> Option<String> {
        fold(self.merged.name.as_ref()).or_else(|| fold(self.merged.model.as_ref()))
    }

    fn absorbs(&self, other: &Device) -> bool {
        if let (Some(mine), Some(theirs)) = (
            fold(self.merged.serial.as_ref()),
            fold(other.serial.as_ref()),
        ) {
            return mine == theirs;
        }
        let label = fold(other.name.as_ref()).or_else(|| fold(other.model.as_ref()));
        self.label().is_some()
            && self.label() == label
            && agree(self.merged.brand.as_ref(), other.brand.as_ref())
    }

    fn absorb(&mut self, other: &Device, at: (usize, usize)) {
        let merged = &mut self.merged;
        for (mine, theirs) in [
            (&mut merged.brand, &other.brand),
            (&mut merged.model, &other.model),
            (&mut merged.serial, &other.serial),
            (&mut merged.firmware, &other.firmware),
            (&mut merged.name, &other.name),
        ] {
            if mine.is_none() {
                mine.clone_from(theirs);
            }
        }
        if merged.dive_number.is_none() {
            merged.dive_number = other.dive_number;
        }
        self.recordings.push(at);
    }

    fn matches(&self, item: &GearItem) -> bool {
        if let (Some(mine), Some(theirs)) = (
            fold(self.merged.serial.as_ref()),
            fold(item.serial.as_ref()),
        ) {
            return mine == theirs;
        }
        self.label().is_some()
            && fold(Some(&item.name)) == self.label()
            && agree(item.brand.as_ref(), self.merged.brand.as_ref())
    }
}

/// The readings that landed on one instant of the profile.
#[derive(Default)]
struct Reading {
    depth: Option<f64>,
    temperature: Option<f64>,
    ndl: Option<f64>,
    ppo2: Option<f64>,
    cns: Option<f64>,
    gradient_factor: Option<f64>,
    pressures: Vec<(String, f64)>,
    marker: Option<String>,
    switch: Option<String>,
}

struct Writer<'a> {
    doc: &'a Document,
    notes: Report,
    mix_ids: Vec<((MixKey, usize), String)>,
    computers: Vec<DeviceRecord>,
    device_links: HashMap<(usize, usize), String>,
    contacts: HashMap<Uuid, &'a Contact>,
    copied: HashSet<Uuid>,
    shared: HashSet<String>,
    linked: HashSet<Uuid>,
    copies: usize,
    people: HashMap<Uuid, &'a Person>,
    guide_ids: Vec<((Uuid, Uuid), String)>,
}

fn gear_element(gear_type: Option<&GearType>) -> u::EquipmentKind {
    use u::EquipmentKind as K;
    match gear_type {
        Some(GearType::Mask) => K::Mask,
        Some(GearType::Fins) => K::Fins,
        Some(GearType::Wetsuit | GearType::Drysuit) => K::Suit,
        Some(GearType::Gloves) => K::Gloves,
        Some(GearType::Boots) => K::Boots,
        Some(GearType::Bcd | GearType::Wing) => K::BuoyancyControlDevice,
        Some(GearType::Regulator) => K::Regulator,
        Some(GearType::Rebreather) => K::Rebreather,
        Some(GearType::Computer) => K::DiveComputer,
        Some(GearType::Cylinder) => K::Tank,
        Some(GearType::Light) => K::Light,
        Some(GearType::Scooter) => K::Scooter,
        Some(GearType::Knife) => K::Knife,
        Some(GearType::Compass) => K::Compass,
        _ => K::VariousPieces,
    }
}

/// Whether a gear type reads back as itself from the element it is written as.
fn round_trips(gear_type: GearType) -> bool {
    !matches!(
        gear_type,
        GearType::Snorkel
            | GearType::Vest
            | GearType::Hood
            | GearType::Backplate
            | GearType::Wing
            | GearType::Transmitter
            | GearType::Smb
            | GearType::Mirror
            | GearType::Whistle
            | GearType::Reel
            | GearType::LineCutter
            | GearType::Shears
            | GearType::Camera
    )
}

fn type_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

fn is_shop(contact: &Contact) -> bool {
    contact.roles == [ContactRole::Shop]
}

impl<'a> Writer<'a> {
    fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            notes: Vec::new(),
            mix_ids: Vec::new(),
            computers: Vec::new(),
            device_links: HashMap::new(),
            contacts: HashMap::new(),
            copied: HashSet::new(),
            shared: HashSet::new(),
            linked: HashSet::new(),
            copies: 0,
            people: HashMap::new(),
            guide_ids: Vec::new(),
        }
    }

    fn note(&mut self, at: &str, message: impl Into<String>, kind: NoteKind) {
        self.notes.push(Note {
            kind,
            code: None,
            at: at.to_owned(),
            member: None,
            message: message.into(),
        });
    }

    /// Reports every member of a record this writer does not place, derived from the
    /// record rather than listed (`docs/writing.md`).
    fn unmapped<T: Serialize>(&mut self, at: &str, record: &T, carried: &[&str]) {
        let Ok(serde_json::Value::Object(members)) = serde_json::to_value(record) else {
            return;
        };
        let mut names: Vec<&String> = members
            .keys()
            .filter(|name| !carried.contains(&name.as_str()))
            .collect();
        names.sort();
        for name in names {
            self.note(
                at,
                format!("UDDF has no slot for {name}; it is not written"),
                NoteKind::Dropped,
            );
        }
    }

    fn notes_of(&mut self, at: &str, notes: Option<&String>) -> Vec<u::NoteItem> {
        match notes {
            Some(text) if !text.is_empty() => para(text),
            Some(_) => {
                self.note(
                    at,
                    "the note is empty, and an empty <para> reads back as no note at all; nothing is written for it",
                    NoteKind::Dropped,
                );
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    fn run(&mut self) -> u::Uddf {
        let doc = self.doc;
        let mut out = u::Uddf::default();
        out.generator.name = Some("divejson".to_owned());
        out.generator.kind = Some(u::GeneratorType::Converter);
        out.generator.version = Some(env!("CARGO_PKG_VERSION").to_owned());
        out.generator.datetime = doc.exported_at.as_str().parse().ok();

        self.plan_mixes();
        self.plan_computers();
        self.plan_contacts();
        self.plan_people();
        self.unmapped(
            "$",
            doc,
            &[
                "format",
                "version",
                "exported_at",
                "generator",
                "extensions",
                "diver",
                "dives",
                "trips",
                "sites",
                "gear",
                "contacts",
                "people",
                "species",
            ],
        );
        if !doc.species.is_empty() {
            self.note(
                "$",
                format!("{NO_SPECIES}; the species are not written"),
                NoteKind::Dropped,
            );
        }
        if !doc.extensions.is_empty() {
            self.note(
                "$",
                "the document's extensions are producer-defined members, and UDDF's <generator> and version describe the file in front of a reader; they are written as this converter's own",
                NoteKind::Dropped,
            );
        }

        for (index, contact) in doc.contacts.iter().enumerate() {
            if is_shop(contact) {
                let (shop, _) = self.contact_element(index, contact, "shop");
                let mut element = u::Shop::default();
                element.id = shop.id;
                element.name = shop.name;
                element.address = shop.address;
                element.contact = shop.contact;
                element.notes = shop.notes;
                out.shops.push(element);
            }
        }
        self.diver_element(&mut out);
        for (index, contact) in doc.contacts.iter().enumerate() {
            if !is_shop(contact) {
                let (base, _) = self.contact_element(index, contact, "divebase");
                out.dive_bases.push(base);
            }
        }
        for (index, site) in doc.sites.iter().enumerate() {
            out.sites.push(self.site_element(index, site));
        }
        for (index, trip) in doc.trips.iter().enumerate() {
            out.trips.push(self.trip_element(index, trip));
        }
        for ((key, _), mix_id) in &self.mix_ids {
            let mut mix = u::Mix::default();
            mix.id = Some(mix_id.clone());
            mix.name = Some(key.name());
            mix.o2 = key.oxygen.map(|o| tidy(o / 100.0));
            mix.he = key.helium.map(|h| tidy(h / 100.0));
            mix.maximum_po2 = key.ppo2_limit;
            out.mixes.push(mix);
        }
        if !doc.dives.is_empty() {
            let mut group = u::RepetitionGroup::default();
            group.id = Some("rg-1".to_owned());
            for (index, dive) in doc.dives.iter().enumerate() {
                group.dives.push(self.dive_element(index, dive));
            }
            out.repetition_groups.push(group);
        }
        out
    }

    // -- diver and people --------------------------------------------------------------

    fn diver_element(&mut self, out: &mut u::Uddf) {
        let doc = self.doc;
        let diver = doc.diver.as_ref();
        let name = diver.and_then(|d| d.name.as_ref());
        let email = diver.and_then(|d| d.email.as_ref());
        let phone = diver.and_then(|d| d.phone.as_ref());
        let born_on = diver.and_then(|d| d.born_on.as_ref());
        let insurances = diver.map(|d| d.insurances.as_slice()).unwrap_or_default();
        let equipment = self.equipment_element();
        let described = name.is_some()
            || email.is_some()
            || phone.is_some()
            || born_on.is_some()
            || !insurances.is_empty();
        if !(described || equipment.is_some() || !doc.people.is_empty()) {
            if diver.is_some() {
                self.note(
                    "diver",
                    "the document records nothing about the logbook's owner that UDDF's <owner> has an element for; no diver is written",
                    NoteKind::Dropped,
                );
            }
            return;
        }
        if let Some(diver) = diver {
            self.unmapped(
                "diver",
                diver,
                &["uuid", "name", "email", "phone", "born_on", "insurances"],
            );
        }
        let mut owner = u::Person::default();
        owner.id = Some(match diver.and_then(|d| d.uuid) {
            Some(uuid) if described => id("diver", uuid),
            _ => "owner".to_owned(),
        });
        let (first, last) = person_names(name.map_or("", String::as_str));
        owner.personal.first_name = Some(first);
        owner.personal.last_name = Some(last);
        owner.personal.birth_date = born_on.copied().and_then(midnight);
        if phone.is_some() || email.is_some() {
            let mut contact = u::Contact::default();
            contact.phones.extend(phone.cloned());
            contact.emails.extend(email.cloned());
            owner.contact = Some(contact);
        }
        if let Some(equipment) = equipment {
            owner.equipment = equipment;
        }
        for (index, insurance) in insurances.iter().enumerate() {
            self.unmapped(
                &format!("diver/insurances/{index}"),
                insurance,
                &["provider", "expires_on"],
            );
            let mut policy = u::Insurance::default();
            policy.name = Some(insurance.provider.clone());
            policy.valid_date = insurance.expires_on.and_then(midnight);
            owner.insurances.push(policy);
        }
        out.owner = Some(owner);
        for (index, person) in doc.people.iter().enumerate() {
            out.buddies.push(self.buddy_element(index, person));
        }
    }

    fn plan_people(&mut self) {
        let doc = self.doc;
        self.people = doc.people.iter().map(|p| (p.uuid, p)).collect();
        for dive in &doc.dives {
            let Some(base) = self.guide_base(dive) else {
                continue;
            };
            for reference in &dive.people {
                if reference.role == Some(PersonRole::Guide)
                    && self.people.contains_key(&reference.person_uuid)
                    && !self
                        .guide_ids
                        .iter()
                        .any(|(key, _)| *key == (base, reference.person_uuid))
                {
                    let guide_id = format!("guide-{}", self.guide_ids.len());
                    self.guide_ids
                        .push(((base, reference.person_uuid), guide_id));
                }
            }
        }
    }

    fn guide_base(&self, dive: &Dive) -> Option<Uuid> {
        let contact = self.contacts.get(&dive.contact_uuid?)?;
        (!is_shop(contact)).then_some(contact.uuid)
    }

    fn buddy_element(&mut self, index: usize, person: &Person) -> u::Buddy {
        let at = format!("people/{index}");
        self.unmapped(&at, person, &["uuid", "name", "email", "phone", "notes"]);
        let mut buddy = u::Buddy::default();
        buddy.person.id = Some(id("person", person.uuid));
        let (first, last) = person_names(&person.name);
        if first.is_empty() {
            self.note(
                &at,
                "the name is whitespace alone, and a reader drops a buddy with no name; the person does not come back, and neither does any reference to it",
                NoteKind::Dropped,
            );
        } else if person.name.split_whitespace().collect::<Vec<_>>().join(" ") != person.name {
            self.note(
                &at,
                "the name's whitespace goes out as one space between its words, which is all <firstname> and <lastname> give back",
                NoteKind::Dropped,
            );
        }
        buddy.person.personal.first_name = Some(first);
        buddy.person.personal.last_name = Some(last);
        if person.phone.is_some() || person.email.is_some() {
            let mut contact = u::Contact::default();
            contact.phones.extend(person.phone.clone());
            contact.emails.extend(person.email.clone());
            buddy.person.contact = Some(contact);
        }
        buddy.person.notes = self.notes_of(&at, person.notes.as_ref());
        buddy
    }

    fn person_link(&mut self, dive: &Dive, reference: &PersonReference, at: &str) -> String {
        self.unmapped(at, reference, &["person_uuid", "role"]);
        let role = reference.role.as_ref();
        let guide = self.guide_base(dive).and_then(|base| {
            self.guide_ids
                .iter()
                .find(|(key, _)| *key == (base, reference.person_uuid))
                .map(|(_, guide_id)| guide_id.clone())
        });
        if role == Some(&PersonRole::Guide)
            && let Some(guide) = guide
        {
            return guide;
        }
        match role {
            None => self.note(
                at,
                "the reference records no role, and the plain link it goes out as reads back as buddy",
                NoteKind::Dropped,
            ),
            Some(PersonRole::Guide) => self.note(
                at,
                "UDDF records a guide under the dive base the dive links, and this dive links none; the reference goes out as a plain link and comes back as buddy",
                NoteKind::Dropped,
            ),
            Some(PersonRole::Buddy) => {}
            Some(other) => self.note(
                at,
                format!("UDDF has no spelling for the role {} on a dive; the reference goes out as a plain link and comes back as buddy", type_name(other)),
                NoteKind::Dropped,
            ),
        }
        id("person", reference.person_uuid)
    }

    // -- computers and gear ------------------------------------------------------------

    fn plan_computers(&mut self) {
        let doc = self.doc;
        for (dive_index, dive) in doc.dives.iter().enumerate() {
            for (rec_index, recording) in dive.recordings.iter().enumerate() {
                let Some(found) = recording.device.as_ref() else {
                    continue;
                };
                let at = (dive_index, rec_index);
                if let Some(record) = self.computers.iter_mut().find(|r| r.absorbs(found)) {
                    record.absorb(found, at);
                } else {
                    self.computers.push(DeviceRecord {
                        merged: found.clone(),
                        recordings: vec![at],
                        gear_index: None,
                        element_id: None,
                        unfolded: Vec::new(),
                    });
                }
            }
        }
        let mut claimed = HashSet::new();
        let mut findings = Vec::new();
        for record in &mut self.computers {
            let candidates: Vec<usize> = doc
                .gear
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    item.gear_type == Some(GearType::Computer) && record.matches(item)
                })
                .map(|(index, _)| index)
                .collect();
            if let Some(free) = candidates.iter().find(|i| !claimed.contains(*i)) {
                record.gear_index = Some(*free);
                claimed.insert(*free);
            }
            let at = record
                .recordings
                .first()
                .map(|(d, r)| format!("dives/{d}/recordings/{r}/device"))
                .unwrap_or_default();
            if record.gear_index.is_none() && !candidates.is_empty() {
                findings.push((at, "the kit item this computer answers to is already another computer's, and a <divecomputer> carries one of each; the first in document order keeps it and this one takes an element of its own"));
            } else if candidates.len() > 1 {
                findings.push((at, "this computer answers to more than one item in the kit list, and a <divecomputer> carries one of each; the first in document order takes it and the rest are written as the kit entries they are, with nothing about the hardware on them"));
            }
        }
        for (at, message) in findings {
            self.note(&at, message, NoteKind::Dropped);
        }
        let mut unfolded = 0_usize;
        for record in &mut self.computers {
            let gear_uuid = record
                .gear_index
                .and_then(|index| doc.gear.get(index))
                .map(|item| item.uuid);
            record.unfolded = record
                .recordings
                .iter()
                .copied()
                .filter(|(dive, _)| {
                    !gear_uuid.is_some_and(|uuid| {
                        doc.dives
                            .get(*dive)
                            .is_some_and(|d| d.gear_uuids.contains(&uuid))
                    })
                })
                .collect();
            if record.unfolded.is_empty() {
                continue;
            }
            let element_id = format!("device-{unfolded}");
            unfolded = unfolded.saturating_add(1);
            for at in &record.unfolded {
                self.device_links.insert(*at, element_id.clone());
            }
            record.element_id = Some(element_id);
        }
    }

    fn device_where(record: &DeviceRecord) -> String {
        record
            .recordings
            .first()
            .map(|(d, r)| format!("dives/{d}/recordings/{r}/device"))
            .unwrap_or_default()
    }

    /// One `<divecomputer>`, from a gear item and a device or from one of them.
    fn computer_element(
        &mut self,
        record: Option<usize>,
        item: Option<(usize, &GearItem)>,
    ) -> u::EquipmentPiece {
        let empty = Device::default();
        let merged = record
            .and_then(|r| self.computers.get(r))
            .map_or(&empty, |r| &r.merged)
            .clone();
        let mut piece = u::EquipmentPiece::default();
        let manufacturer_id;
        if let Some((_, item)) = item {
            piece.id = Some(id("gear", item.uuid));
            manufacturer_id = id("mfr", item.uuid);
        } else {
            let element_id = record
                .and_then(|r| self.computers.get(r))
                .and_then(|r| r.element_id.clone())
                .unwrap_or_default();
            manufacturer_id = format!("mfr-{element_id}");
            piece.id = Some(element_id);
        }
        let mut placed = vec!["dive_number"];
        let name = item.map(|(_, i)| i.name.clone()).or(merged.name.clone());
        if name == merged.name {
            placed.push("name");
        }
        piece.name = name;
        let brand = item
            .and_then(|(_, i)| i.brand.clone())
            .or(merged.brand.clone());
        if let Some(brand) = &brand {
            let mut manufacturer = u::Manufacturer::default();
            manufacturer.id = Some(manufacturer_id);
            manufacturer.name = Some(brand.clone());
            piece.manufacturer = Some(manufacturer);
        }
        if brand == merged.brand {
            placed.push("brand");
        }
        if merged.model.is_some() {
            piece.model.clone_from(&merged.model);
            placed.push("model");
        }
        let serial = merged
            .serial
            .clone()
            .or_else(|| item.and_then(|(_, i)| i.serial.clone()));
        if serial == merged.serial {
            placed.push("serial");
        }
        piece.serial_number = serial;
        if let Some((index, item)) = item {
            piece.notes = self.notes_of(&format!("gear/{index}"), item.notes.as_ref());
        }
        if merged != empty
            && let Some(record) = record.and_then(|r| self.computers.get(r))
        {
            let at = Self::device_where(record);
            self.unmapped(&at, &merged, &placed);
        }
        piece
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every gear type onto its equipment element"
    )]
    fn equipment_element(&mut self) -> Option<u::Equipment> {
        let doc = self.doc;
        let loose: Vec<usize> = self
            .computers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.element_id.is_some())
            .map(|(i, _)| i)
            .collect();
        if doc.gear.is_empty() && loose.is_empty() {
            return None;
        }
        let folded: HashMap<usize, usize> = self
            .computers
            .iter()
            .enumerate()
            .filter_map(|(r, record)| {
                let index = record.gear_index?;
                (record.unfolded.len() < record.recordings.len()).then_some((index, r))
            })
            .collect();
        let mut equipment = u::Equipment::default();
        for (index, item) in doc.gear.iter().enumerate() {
            let at = format!("gear/{index}");
            let kind = gear_element(item.gear_type.as_ref());
            let mut carried = vec!["uuid", "name", "brand", "serial", "type", "notes"];
            if kind == u::EquipmentKind::Tank {
                carried.extend(["volume", "material"]);
            }
            self.unmapped(&at, item, &carried);
            match &item.gear_type {
                None => self.note(
                    &at,
                    "the piece records no type, and every UDDF equipment element is one; it is written as <variouspieces>, which reads back as 'other'",
                    NoteKind::Absent,
                ),
                Some(gear_type) if !round_trips(*gear_type) => {
                    let back = match kind {
                        u::EquipmentKind::BuoyancyControlDevice => "bcd",
                        _ => "other",
                    };
                    self.note(
                        &at,
                        format!("UDDF's equipment vocabulary does not name {:?}; the piece is written as <{}>, which reads back as {back:?}", type_name(gear_type), kind.element()),
                        NoteKind::Dropped,
                    );
                }
                Some(_) => {}
            }
            if kind == u::EquipmentKind::DiveComputer {
                let piece = self.computer_element(folded.get(&index).copied(), Some((index, item)));
                equipment.dive_computers.push(piece);
                continue;
            }
            let mut piece = u::EquipmentPiece::default();
            piece.id = Some(id("gear", item.uuid));
            piece.name = Some(item.name.clone());
            if let Some(brand) = &item.brand {
                let mut manufacturer = u::Manufacturer::default();
                manufacturer.id = Some(id("mfr", item.uuid));
                manufacturer.name = Some(brand.clone());
                piece.manufacturer = Some(manufacturer);
            }
            piece.serial_number.clone_from(&item.serial);
            piece.notes = self.notes_of(&at, item.notes.as_ref());
            match kind {
                u::EquipmentKind::Suit => {
                    let mut suit = u::Suit::default();
                    suit.piece = piece;
                    suit.suit_type = match item.gear_type {
                        Some(GearType::Wetsuit) => Some(u::SuitType::WetSuit),
                        Some(GearType::Drysuit) => Some(u::SuitType::DrySuit),
                        _ => None,
                    };
                    equipment.suits.push(suit);
                }
                u::EquipmentKind::Tank => {
                    let mut tank = u::Tank::default();
                    tank.piece = piece;
                    tank.material = item.material.as_ref().map(|m| match m {
                        Material::Aluminium => u::TankMaterial::Aluminium,
                        Material::Carbon => u::TankMaterial::Carbon,
                        Material::Steel => u::TankMaterial::Steel,
                    });
                    tank.volume = item.volume.map(|v| tidy(v / LITRES_PER_CUBIC_METRE));
                    equipment.tanks.push(tank);
                }
                u::EquipmentKind::Rebreather => {
                    let mut rebreather = u::Rebreather::default();
                    rebreather.piece = piece;
                    equipment.rebreathers.push(rebreather);
                }
                kind => {
                    let list = match kind {
                        u::EquipmentKind::Boots => &mut equipment.boots,
                        u::EquipmentKind::BuoyancyControlDevice => {
                            &mut equipment.buoyancy_control_devices
                        }
                        u::EquipmentKind::Compass => &mut equipment.compasses,
                        u::EquipmentKind::Fins => &mut equipment.fins,
                        u::EquipmentKind::Gloves => &mut equipment.gloves,
                        u::EquipmentKind::Knife => &mut equipment.knives,
                        u::EquipmentKind::Light => &mut equipment.lights,
                        u::EquipmentKind::Mask => &mut equipment.masks,
                        u::EquipmentKind::Regulator => &mut equipment.regulators,
                        u::EquipmentKind::Scooter => &mut equipment.scooters,
                        _ => &mut equipment.various_pieces,
                    };
                    list.push(piece);
                }
            }
        }
        for record in loose {
            let piece = self.computer_element(Some(record), None);
            equipment.dive_computers.push(piece);
        }
        Some(equipment)
    }

    // -- contacts ----------------------------------------------------------------------

    fn plan_contacts(&mut self) {
        let doc = self.doc;
        self.contacts = doc.contacts.iter().map(|c| (c.uuid, c)).collect();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for contact in &doc.contacts {
            if let Some(name) = fold(Some(&contact.name)) {
                let count = counts.entry(name).or_insert(0);
                *count = count.saturating_add(1);
            }
        }
        self.shared = counts
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(name, _)| name)
            .collect();
        let copied: HashSet<Uuid> = doc
            .trips
            .iter()
            .flat_map(|trip| trip.parts.iter())
            .filter_map(|part| self.stay_of(part).map(|c| c.uuid))
            .collect();
        self.copied = copied;
        self.linked = doc.dives.iter().filter_map(|d| d.contact_uuid).collect();
    }

    fn stay_of(&self, part: &crate::TripPart) -> Option<&'a Contact> {
        let contact = *self.contacts.get(&part.accommodation_uuid?)?;
        (!fold(Some(&contact.name)).is_some_and(|name| self.shared.contains(&name)))
            .then_some(contact)
    }

    /// A contact as a `<divebase>` (or the parts of a `<shop>`), and whether it carries
    /// anything beside its name.
    fn contact_element(
        &mut self,
        index: usize,
        contact: &Contact,
        tag: &str,
    ) -> (u::DiveBase, bool) {
        let at = format!("contacts/{index}");
        self.unmapped(
            &at,
            contact,
            &[
                "uuid", "name", "roles", "phone", "email", "website", "address", "notes",
            ],
        );
        let mut base = u::DiveBase::default();
        base.id = Some(id("contact", contact.uuid));
        base.name = Some(contact.name.clone());
        let (address, block) = self.contact_contents(contact, &at, true);
        let mut carries = address.is_some() || block.is_some();
        base.address = address;
        base.contact = block;
        if tag == "divebase" {
            for ((owner, person), guide_id) in &self.guide_ids {
                if *owner == contact.uuid {
                    let mut guide = u::Guide::default();
                    guide.id = Some(guide_id.clone());
                    guide.link = Some(id("person", *person));
                    base.guides.push(guide);
                }
            }
        }
        base.notes = self.notes_of(&at, contact.notes.as_ref());
        carries |= !base.notes.is_empty();

        let mut implied = vec![if tag == "shop" {
            ContactRole::Shop
        } else {
            ContactRole::DiveCenter
        }];
        if self.copied.contains(&contact.uuid) {
            implied.push(ContactRole::Accommodation);
        }
        let same = contact.roles.len() == implied.len()
            && implied.iter().all(|role| contact.roles.contains(role));
        if !same {
            let said = implied.iter().map(type_name).collect::<Vec<_>>().join(", ");
            let message = if contact.roles.is_empty() {
                format!(
                    "UDDF has no element for roles, and the contact records none; the <{tag}> it goes out as reads back as {said}"
                )
            } else {
                format!(
                    "UDDF has no element for roles, and the slots the contact goes out in say only {said} back; its roles are not written"
                )
            };
            self.note(&at, message, NoteKind::Dropped);
        }
        if tag == "divebase"
            && !carries
            && !self.linked.contains(&contact.uuid)
            && !self.copied.contains(&contact.uuid)
        {
            self.note(
                &at,
                "the contact carries nothing UDDF holds but its name, and no dive links it and no part's copy names it — the shape a reader skips as a writer's placeholder <divebase>; it does not come back",
                NoteKind::Dropped,
            );
        }
        (base, carries)
    }

    /// A contact's `<address>` and `<contact>` block.
    fn contact_contents(
        &mut self,
        contact: &Contact,
        at: &str,
        report: bool,
    ) -> (Option<u::Address>, Option<u::Contact>) {
        let mut address = None;
        if let Some(source) = &contact.address {
            if report {
                self.unmapped(
                    &format!("{at}/address"),
                    source,
                    &["street", "city", "postcode", "region", "country"],
                );
            }
            if source.country.is_empty() {
                if report {
                    self.note(
                        at,
                        "the address records no country, which UDDF's <address> requires; it is not written",
                        NoteKind::Dropped,
                    );
                }
            } else {
                let mut out = u::Address::default();
                out.street.clone_from(&source.street);
                out.city.clone_from(&source.city);
                out.postcode.clone_from(&source.postcode);
                out.country = Some(source.country.clone());
                out.province.clone_from(&source.region);
                address = Some(out);
            }
        }
        let mut block = None;
        if contact.phone.is_some() || contact.email.is_some() || contact.website.is_some() {
            let mut out = u::Contact::default();
            out.phones.extend(contact.phone.clone());
            out.emails.extend(contact.email.clone());
            out.homepages.extend(contact.website.clone());
            block = Some(out);
        }
        (address, block)
    }

    // -- sites -------------------------------------------------------------------------

    fn site_element(&mut self, index: usize, site: &crate::DiveSite) -> u::Site {
        let at = format!("sites/{index}");
        self.unmapped(
            &at,
            site,
            &[
                "uuid",
                "name",
                "other_names",
                "location",
                "position",
                "altitude",
                "depth_from",
                "depth_to",
                "notes",
            ],
        );
        let mut element = u::Site::default();
        element.id = Some(id("site", site.uuid));
        element.name = Some(site.name.clone());
        element.alias_names.clone_from(&site.other_names);
        if let Some(location) = &site.location {
            self.unmapped(&format!("{at}/location"), location, &["name"]);
        }
        element.geography = self.geography(
            &at,
            site.location.as_ref().map(|l| l.name.as_str()),
            site.position.as_ref(),
            site.altitude,
            "site",
        );
        if site.depth_from.is_some() || site.depth_to.is_some() {
            let mut data = u::SiteData::default();
            data.maximum_depth = site.depth_to;
            data.minimum_depth = site.depth_from;
            element.site_data = Some(data);
        }
        element.notes = self.notes_of(&at, site.notes.as_ref());
        element
    }

    fn geography(
        &mut self,
        at: &str,
        place: Option<&str>,
        position: Option<&crate::Position>,
        altitude: Option<i32>,
        noun: &str,
    ) -> Option<u::Geography> {
        let Some(place) = place.filter(|p| !p.is_empty()) else {
            let lost: Vec<&str> = [position.map(|_| "position"), altitude.map(|_| "altitude")]
                .into_iter()
                .flatten()
                .collect();
            if !lost.is_empty() {
                let verb = if lost.len() > 1 { "are" } else { "is" };
                self.note(
                    at,
                    format!("UDDF records a position or an altitude only inside a <geography>, which must name a place, and the {noun} records none; the {} {verb} dropped rather than the name being copied into it", lost.join(" and the ")),
                    NoteKind::Dropped,
                );
            }
            return None;
        };
        let mut geography = u::Geography::default();
        geography.location = Some(place.to_owned());
        geography.latitude = position.map(|p| p.latitude);
        geography.longitude = position.map(|p| p.longitude);
        geography.altitude = altitude.map(f64::from);
        Some(geography)
    }

    // -- trips -------------------------------------------------------------------------

    fn trip_element(&mut self, index: usize, trip: &crate::Trip) -> u::Trip {
        let at = format!("trips/{index}");
        self.unmapped(&at, trip, &["uuid", "name", "parts", "notes"]);
        let mut element = u::Trip::default();
        element.id = Some(id("trip", trip.uuid));
        element.name = Some(trip.name.clone());
        let parts: Vec<Option<&crate::TripPart>> = if trip.parts.is_empty() {
            vec![None]
        } else {
            trip.parts.iter().map(Some).collect()
        };
        for (part_index, record) in parts.into_iter().enumerate() {
            let part_at = format!("{at}/parts/{part_index}");
            let mut part = u::TripPart::default();
            let location = record.and_then(|r| r.location.as_ref());
            if let Some(record) = record {
                self.unmapped(
                    &part_at,
                    record,
                    &["starts_on", "ends_on", "location", "accommodation_uuid"],
                );
            }
            if let Some(location) = location {
                self.unmapped(&part_at, location, &["name", "position"]);
                part.name = Some(location.name.clone());
            } else {
                part.name = Some(String::new());
                if let Some(record) = record {
                    self.nameless_part(&part_at, record);
                }
            }
            if let Some(record) = record {
                self.date_of_trip(&mut part, &part_at, record);
            }
            if let Some(location) = location {
                part.geography = self.geography(
                    &part_at,
                    Some(&location.name),
                    location.position.as_ref(),
                    None,
                    "location",
                );
            }
            if let Some(record) = record {
                self.accommodation(&mut part, &part_at, record);
            }
            if part_index == 0 {
                part.notes = self.notes_of(&at, trip.notes.as_ref());
            }
            element.parts.push(part);
        }
        element
    }

    fn nameless_part(&mut self, at: &str, record: &crate::TripPart) {
        let message = if record.starts_on.is_some()
            || record.ends_on.is_some()
            || self.stay_of(record).is_some()
        {
            "the part records no place, and UDDF's <trippart> requires a <name>; an empty one is written, which reads back as the placeless part it is"
        } else {
            "the part records neither a place, a date nor a stay UDDF can carry, and UDDF's <trippart> requires a <name>; an empty one is written, which is the element a trip with no parts is written as and reads back as no part at all"
        };
        self.note(at, message, NoteKind::Absent);
    }

    fn date_of_trip(&mut self, part: &mut u::TripPart, at: &str, record: &crate::TripPart) {
        let (starts, ends) = match (&record.starts_on, &record.ends_on) {
            (None, None) => return,
            (Some(starts), None) => {
                self.note(
                    at,
                    "the part records no end date, and UDDF's <dateoftrip> requires both; the start date is written into both, so a reader sees a stretch that began and ended on one day",
                    NoteKind::Absent,
                );
                (starts, starts)
            }
            (None, Some(ends)) => {
                self.note(
                    at,
                    "the part records no start date, and UDDF's <dateoftrip> requires both; the end date is written into both, so a reader sees a stretch that began and ended on one day",
                    NoteKind::Absent,
                );
                (ends, ends)
            }
            (Some(starts), Some(ends)) => (starts, ends),
        };
        part.start_date = midnight(*starts);
        part.end_date = midnight(*ends);
    }

    fn accommodation(&mut self, part: &mut u::TripPart, at: &str, record: &crate::TripPart) {
        let Some(uuid) = record.accommodation_uuid else {
            return;
        };
        let Some(contact) = self.stay_of(record) else {
            let message = if self.contacts.contains_key(&uuid) {
                "the part's contact shares its name with another contact, and a reader folds a copy back into a contact by name alone, so a copy would come back as whichever it met first; no copy is written and accommodation_uuid is not carried"
            } else {
                "accommodation_uuid names no contact in the document; it is not written"
            };
            self.note(at, message, NoteKind::Dropped);
            return;
        };
        let mut copy = u::Accommodation::default();
        copy.id = Some(format!("accommodation-{}", self.copies));
        self.copies = self.copies.saturating_add(1);
        copy.name = Some(contact.name.clone());
        let (address, block) = self.contact_contents(contact, at, false);
        copy.address = address;
        copy.contact = block;
        if let Some(notes) = contact.notes.as_ref().filter(|n| !n.is_empty()) {
            copy.notes = para(notes);
        }
        part.accommodation = Some(copy);
        part.kind = Some(u::TripPartType::from_uddf(
            if contact.roles.contains(&ContactRole::Liveaboard) {
                "boat"
            } else {
                "hotel"
            },
        ));
    }

    // -- gases -------------------------------------------------------------------------

    fn plan_mixes(&mut self) {
        let mut keys: Vec<(MixKey, usize)> = Vec::new();
        for dive in &self.doc.dives {
            let mut seen: Vec<(MixKey, usize)> = Vec::new();
            for cylinder in &dive.cylinders {
                let key = MixKey::of(cylinder);
                let occurrence = occurrence(&mut seen, key);
                if !keys.contains(&(key, occurrence)) {
                    keys.push((key, occurrence));
                }
            }
        }
        keys.sort_by(|(a, i), (b, j)| {
            let (a, b) = (a.sort_key(), b.sort_key());
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| x.total_cmp(y))
                .find(|o| o.is_ne())
                .unwrap_or(core::cmp::Ordering::Equal)
                .then(i.cmp(j))
        });
        self.mix_ids = keys
            .into_iter()
            .enumerate()
            .map(|(n, key)| (key, format!("mix-{}", n.saturating_add(1))))
            .collect();
    }

    fn mix_id(&self, key: MixKey, occurrence: usize) -> String {
        self.mix_ids
            .iter()
            .find(|(k, _)| *k == (key, occurrence))
            .map(|(_, mix_id)| mix_id.clone())
            .unwrap_or_default()
    }

    // -- dives -------------------------------------------------------------------------

    #[allow(
        clippy::too_many_lines,
        reason = "one dive's elements, in the XSD's order"
    )]
    fn dive_element(&mut self, index: usize, dive: &Dive) -> u::Dive {
        let at = format!("dives/{index}");
        self.unmapped(
            &at,
            dive,
            &[
                "uuid",
                "number",
                "started_at",
                "submerged_time",
                "notes",
                "type",
                "rating",
                "max_depth",
                "avg_depth",
                "bottom_temperature",
                "visibility",
                "weight",
                "altitude",
                "air_temperature",
                "current",
                "entry_type",
                "trip_uuid",
                "contact_uuid",
                "site_uuids",
                "gear_uuids",
                "people",
                "sightings",
                "cylinders",
                "recordings",
            ],
        );
        if !dive.sightings.is_empty() {
            self.note(
                &format!("{at}/sightings"),
                format!("{NO_SPECIES}; the sightings are not written"),
                NoteKind::Dropped,
            );
        }
        let recordings = &dive.recordings;
        let mut element = u::Dive::default();
        element.id = Some(id("dive", dive.uuid));
        let before = &mut element.information_before_dive;
        before
            .links
            .extend(dive.site_uuids.iter().map(|uuid| id("site", *uuid)));
        if let Some(contact) = dive.contact_uuid {
            before.links.push(id("contact", contact));
        }
        for (position, reference) in dive.people.iter().enumerate() {
            let link = self.person_link(dive, reference, &format!("{at}/people/{position}"));
            element.information_before_dive.links.push(link);
        }
        if let Some(number) = dive.number {
            if let Some(number) = u32::try_from(number).ok().filter(|n| *n > 0) {
                element.information_before_dive.dive_number = Some(number);
            } else {
                self.note(
                    &at,
                    format!("the dive is numbered {number}, and UDDF's <divenumber> is a positive integer; the number is not written"),
                    NoteKind::Dropped,
                );
            }
        }
        element.information_before_dive.internal_dive_number =
            self.internal_dive_number(recordings, &at);
        let before = &mut element.information_before_dive;
        before.datetime = dive.started_at.to_string().parse().ok();
        before.air_temperature = dive.air_temperature.map(|c| tidy(c + KELVIN_OFFSET));
        before.altitude = dive.altitude.map(f64::from);
        let devices: Vec<String> = (0..recordings.len())
            .filter_map(|r| self.device_links.get(&(index, r)).cloned())
            .collect();
        if dive.weight.is_some() || !dive.gear_uuids.is_empty() || !devices.is_empty() {
            let mut used = u::EquipmentUsed::default();
            used.lead_quantity = dive.weight;
            used.links
                .extend(dive.gear_uuids.iter().map(|uuid| id("gear", *uuid)));
            for device in &devices {
                if !used.links.contains(device) {
                    used.links.push(device.clone());
                }
            }
            before.equipment_used = Some(used);
        }
        self.check_link_order(index, dive, &devices, &at);
        if let Some(dive_type) = &dive.dive_type {
            let written = match dive_type {
                DiveType::OpenCircuit => Some("open-scuba"),
                DiveType::ClosedCircuit | DiveType::SemiClosed => Some("rebreather"),
                DiveType::SurfaceSupplied => Some("surface-supplied"),
                _ => None,
            };
            let back = (written == Some("rebreather")).then_some("closed_circuit");
            element.information_before_dive.apparatus = self
                .enumeration("apparatus", dive_type, written, back, &format!("{at}/type"))
                .map(u::Apparatus::from_uddf);
        }
        if let Some(entry_type) = &dive.entry_type {
            let written = match entry_type {
                EntryType::Shore => Some("beach-shore"),
                EntryType::Pier => Some("pier"),
                _ => None,
            };
            element.information_before_dive.platform = self
                .enumeration(
                    "platform",
                    entry_type,
                    written,
                    None,
                    &format!("{at}/entry_type"),
                )
                .map(u::Platform::from_uddf);
        }
        let before = &mut element.information_before_dive;
        before.trip_membership = dive.trip_uuid.map(|uuid| id("trip", uuid));
        before.surface_pressure = recordings
            .first()
            .and_then(|r| r.surface_pressure)
            .map(|bar| tidy(bar * PASCAL_PER_BAR));

        let mix_by_gas_number = self.tankdata_elements(&mut element, dive, &at);
        let numbered = self.recording_elements(&mut element, recordings, &at, &mix_by_gas_number);
        let declared: Vec<Option<u32>> = dive.cylinders.iter().map(|c| c.gas_number).collect();
        let recovered: Vec<Option<u32>> = (0..declared.len())
            .map(|n| numbered.then(|| u32::try_from(n).ok()).flatten())
            .collect();
        if declared != recovered && declared.iter().any(Option::is_some) {
            self.note(
                &at,
                "UDDF records no cylinder numbering, so a reader recovers one by counting this dive's <tankdata> elements, and only where its profile needs one; the gas numbers the document carries are not preserved (spec §6.3)",
                NoteKind::Dropped,
            );
        }

        let after = &mut element.information_after_dive;
        after.lowest_temperature = dive.bottom_temperature.map(|c| tidy(c + KELVIN_OFFSET));
        after.greatest_depth = Some(dive.max_depth.unwrap_or(0.0));
        after.visibility = dive.visibility;
        let no_depth = dive.max_depth.is_none();
        if no_depth {
            self.note(
                &at,
                "the dive records no maximum depth, and UDDF's <greatestdepth> is mandatory; 0 is written, which readers of this format take as not recorded",
                NoteKind::Absent,
            );
        }
        if let Some(current) = &dive.current {
            let written = match current {
                Current::None => Some("no-current"),
                Current::Light => Some("mild-current"),
                Current::Moderate => Some("moderate-current"),
                Current::Strong => Some("hard-current"),
                Current::Extreme => Some("very-hard-current"),
            };
            element.information_after_dive.current = self
                .enumeration("current", current, written, None, &format!("{at}/current"))
                .map(u::Current::from_uddf);
        }
        let notes = self.notes_of(&at, dive.notes.as_ref());
        let after = &mut element.information_after_dive;
        after.notes = notes;
        if let Some(rating) = dive.rating {
            let mut value = u::Rating::default();
            value.value = Some(rating.saturating_mul(2));
            after.rating = Some(value);
        }
        #[allow(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "a dive's seconds are far below 2^53"
        )]
        let duration = dive.submerged_time.map_or(0.0, |s| s as f64);
        after.dive_duration = Some(duration);
        after.average_depth = dive.avg_depth;
        if dive.submerged_time.is_none() {
            self.note(
                &at,
                "the dive records no submerged time, and UDDF's <diveduration> is mandatory; 0 is written, which readers of this format take as not recorded",
                NoteKind::Absent,
            );
        }
        element
    }

    /// One of §6.2's vocabularies as a UDDF enumeration, or the report of why it is not.
    fn enumeration<'v, T: Serialize>(
        &mut self,
        tag: &str,
        value: &T,
        written: Option<&'v str>,
        reads_back_as: Option<&str>,
        at: &str,
    ) -> Option<&'v str> {
        let name = type_name(value);
        let Some(element) = written else {
            self.note(
                at,
                format!("UDDF's <{tag}> does not name {name:?}; it is not written"),
                NoteKind::Dropped,
            );
            return None;
        };
        if let Some(back) = reads_back_as.filter(|back| *back != name) {
            self.note(
                at,
                format!("UDDF's <{tag}> does not name {name:?}; it is written as {element:?}, which reads back as {back:?}"),
                NoteKind::Dropped,
            );
        }
        Some(element)
    }

    fn internal_dive_number(&mut self, recordings: &[Recording], at: &str) -> Option<u32> {
        let mut written = None;
        for (index, recording) in recordings.iter().enumerate() {
            let Some(counter) = recording.device.as_ref().and_then(|d| d.dive_number) else {
                continue;
            };
            let here = format!("{at}/recordings/{index}/device");
            if index > 0 {
                self.note(
                    &here,
                    "UDDF records one dive counter per dive, under <informationbeforedive>, and it is the first recording's; this device's counter has nowhere to go",
                    NoteKind::Dropped,
                );
            } else if let Some(counter) = u32::try_from(counter).ok().filter(|c| *c > 0) {
                written = Some(counter);
            } else {
                self.note(
                    &here,
                    format!("the device's counter is {counter}, and UDDF's <internaldivenumber> is a positive integer; the counter is not written"),
                    NoteKind::Dropped,
                );
            }
        }
        written
    }

    fn recording_element_id(&self, dive: usize, recording: usize) -> Option<String> {
        if let Some(element) = self.device_links.get(&(dive, recording)) {
            return Some(element.clone());
        }
        self.computers
            .iter()
            .find(|r| r.recordings.contains(&(dive, recording)))
            .and_then(|r| r.gear_index)
            .and_then(|g| self.doc.gear.get(g))
            .map(|item| id("gear", item.uuid))
    }

    fn check_link_order(&mut self, index: usize, dive: &Dive, devices: &[String], at: &str) {
        let doc = self.doc;
        let computers: HashSet<Uuid> = doc
            .gear
            .iter()
            .filter(|item| item.gear_type == Some(GearType::Computer))
            .map(|item| item.uuid)
            .collect();
        let mut linked: Vec<String> = Vec::new();
        for element in dive
            .gear_uuids
            .iter()
            .filter(|uuid| computers.contains(uuid))
            .map(|uuid| id("gear", *uuid))
            .chain(devices.iter().cloned())
        {
            if !linked.contains(&element) {
                linked.push(element);
            }
        }
        let mut carried: Vec<String> = Vec::new();
        for (r, recording) in dive.recordings.iter().enumerate() {
            if recording.device.is_some()
                && let Some(element) = self.recording_element_id(index, r)
                && !carried.contains(&element)
            {
                carried.push(element);
            }
        }
        let (Some(first_linked), Some(first_carried)) = (linked.first(), carried.first()) else {
            return;
        };
        if first_linked != first_carried {
            self.note(
                at,
                "UDDF gives a dive one <samples> and one <internaldivenumber>, and a reader takes both off the first <divecomputer> the dive links — which is not the element this dive's primary recording was written into; the profile and the device counter come back on that computer instead",
                NoteKind::Dropped,
            );
            return;
        }
        let met: Vec<&String> = linked.iter().filter(|e| carried.contains(e)).collect();
        if !met.iter().copied().eq(carried.iter()) {
            self.note(
                at,
                "a reader recovers this dive's recordings from its <equipmentused> links, which run in the kit list's order rather than the recordings' own; this dive's come back in a different order, and §6.4a makes the first of them the primary",
                NoteKind::Dropped,
            );
        }
    }

    fn tankdata_elements(
        &mut self,
        element: &mut u::Dive,
        dive: &Dive,
        at: &str,
    ) -> HashMap<u32, String> {
        let mut seen: Vec<(MixKey, usize)> = Vec::new();
        let mut by_number: HashMap<u32, String> = HashMap::new();
        for (index, cylinder) in dive.cylinders.iter().enumerate() {
            let cylinder_at = format!("{at}/cylinders/{index}");
            self.unmapped(
                &cylinder_at,
                cylinder,
                &[
                    "volume",
                    "start_pressure",
                    "end_pressure",
                    "oxygen",
                    "helium",
                    "ppo2_limit",
                    "gas_number",
                    "gear_uuid",
                ],
            );
            let key = MixKey::of(cylinder);
            let occurrence = occurrence(&mut seen, key);
            let mix_id = self.mix_id(key, occurrence);
            let number = cylinder
                .gas_number
                .or_else(|| u32::try_from(index).ok())
                .unwrap_or(u32::MAX);
            by_number.entry(number).or_insert_with(|| mix_id.clone());

            let mut tank = u::TankData::default();
            tank.links.push(mix_id);
            if let Some(gear) = cylinder.gear_uuid {
                tank.links.push(id("gear", gear));
            }
            tank.tank_volume = cylinder.volume.map(|v| tidy(v / LITRES_PER_CUBIC_METRE));
            if cylinder.start_pressure.is_none() {
                self.note(
                    &cylinder_at,
                    "the cylinder records no start pressure, and UDDF's <tankpressurebegin> is mandatory; 0 is written, which readers of this format take as not recorded (spec §6.3)",
                    NoteKind::Absent,
                );
            }
            tank.tank_pressure_begin = Some(
                cylinder
                    .start_pressure
                    .map_or(0.0, |p| tidy(p * PASCAL_PER_BAR)),
            );
            tank.tank_pressure_end = cylinder.end_pressure.map(|p| tidy(p * PASCAL_PER_BAR));
            element.tank_data.push(tank);
        }
        by_number
    }

    fn recording_elements(
        &mut self,
        element: &mut u::Dive,
        recordings: &[Recording],
        at: &str,
        mixes: &HashMap<u32, String>,
    ) -> bool {
        let mut numbered = false;
        for (index, recording) in recordings.iter().enumerate() {
            let here = format!("{at}/recordings/{index}");
            self.unmapped(
                &here,
                recording,
                &[
                    "device",
                    "mode",
                    "deco_model",
                    "surface_pressure",
                    "profile",
                ],
            );
            if recording.deco_model.is_some() {
                self.note(
                    &format!("{here}/deco_model"),
                    "UDDF's <decomodel> requires a tissue table — a half-time and its coefficients for every compartment — which §6.4c has no member for, and nothing is invented to satisfy a required element; the decompression model is not written",
                    NoteKind::Dropped,
                );
            }
            if index == 0 {
                numbered = self.samples_element(element, recording, &here, mixes);
            } else {
                self.note(
                    &here,
                    "UDDF holds one profile per dive, and this dive's is the primary recording's; this recording is dropped, its device kept under <equipment> (spec §6.4a)",
                    NoteKind::Dropped,
                );
            }
        }
        numbered
    }

    fn divemode(&mut self, mode: Option<&RecordingMode>, at: &str) -> Option<u::DiveMode> {
        let written = match mode? {
            RecordingMode::OpenCircuit => u::DiveMode::OpenCircuit,
            RecordingMode::ClosedCircuit => u::DiveMode::ClosedCircuit,
            RecordingMode::SemiClosed => u::DiveMode::SemiClosedCircuit,
            RecordingMode::Freedive => u::DiveMode::Apnoe,
            other => {
                self.note(
                    &format!("{at}/mode"),
                    format!("UDDF's <divemode> has no value for a {} recording, its five being open, closed and semi-closed circuit and two spellings of a freedive; the mode is not written", type_name(other)),
                    NoteKind::Dropped,
                );
                return None;
            }
        };
        Some(written)
    }

    fn mode_unplaced(&mut self, mode: Option<&u::DiveMode>, at: &str) {
        if mode.is_some() {
            self.note(
                &format!("{at}/mode"),
                "UDDF states a dive's mode on a waypoint, and this recording has no samples to carry one; the mode is not written",
                NoteKind::Dropped,
            );
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "every channel onto the union of their instants"
    )]
    fn samples_element(
        &mut self,
        element: &mut u::Dive,
        recording: &Recording,
        at: &str,
        mixes: &HashMap<u32, String>,
    ) -> bool {
        let divemode = self.divemode(recording.mode.as_ref(), at);
        let Some(profile) = &recording.profile else {
            self.mode_unplaced(divemode.as_ref(), at);
            return false;
        };
        let profile_at = format!("{at}/profile");
        self.unmapped(
            &profile_at,
            profile,
            &[
                "duration",
                "depth",
                "temperature",
                "pressures",
                "ndl",
                "ppo2",
                "cns",
                "gradient_factor",
                "events",
            ],
        );
        let mut readings: BTreeMap<u64, Reading> = BTreeMap::new();
        #[allow(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "channel values are far below 2^53"
        )]
        let mut fill =
            |series: Option<&Series>, set: fn(&mut Reading, f64), scale: fn(f64) -> f64| {
                for (time, value) in series
                    .iter()
                    .flat_map(|s| s.times.iter().zip(s.values.iter()))
                {
                    set(
                        readings.entry(*time).or_default(),
                        tidy(scale(*value as f64)),
                    );
                }
            };
        fill(
            profile.depth.as_ref(),
            |r, v| r.depth = Some(v),
            |v| v / 100.0,
        );
        fill(
            profile.temperature.as_ref(),
            |r, v| r.temperature = Some(v),
            |v| v / 100.0 + KELVIN_OFFSET,
        );
        fill(profile.ndl.as_ref(), |r, v| r.ndl = Some(v), |v| v);
        fill(
            profile.ppo2.as_ref(),
            |r, v| r.ppo2 = Some(v),
            |v| v / 100.0,
        );
        fill(profile.cns.as_ref(), |r, v| r.cns = Some(v), |v| v / 10.0);
        fill(
            profile.gradient_factor.as_ref(),
            |r, v| r.gradient_factor = Some(v),
            |v| v / 100.0,
        );
        for channel in &profile.pressures {
            let Some(mix_id) = mixes.get(&channel.gas_number) else {
                self.note(
                    &profile_at,
                    format!("the profile carries a pressure channel for gas number {}, which no cylinder of this dive is numbered; the channel is dropped", channel.gas_number),
                    NoteKind::Dropped,
                );
                continue;
            };
            for (time, tenths) in channel.times.iter().zip(channel.values.iter()) {
                #[allow(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "tenths of a bar are far below 2^53"
                )]
                let pascal = tidy(*tenths as f64 / 10.0 * PASCAL_PER_BAR);
                readings
                    .entry(*time)
                    .or_default()
                    .pressures
                    .push((mix_id.clone(), pascal));
            }
        }
        self.events(profile, &profile_at, mixes, &mut readings);

        let Some(first) = readings.keys().next().copied() else {
            self.mode_unplaced(divemode.as_ref(), at);
            return false;
        };
        let mut numbered = false;
        for (instant, reading) in readings {
            let mut waypoint = u::Waypoint::default();
            waypoint.cns = reading.cns;
            waypoint.calculated_po2 = reading.ppo2;
            waypoint.depth = reading.depth;
            #[allow(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "an axis instant is far below 2^53"
            )]
            let seconds = tidy(instant as f64 / 1000.0);
            waypoint.dive_time = Some(seconds);
            waypoint.set_marker = reading.marker;
            numbered |= reading.switch.is_some() || !reading.pressures.is_empty();
            waypoint.switch_mix = reading.switch;
            for (mix_id, pascal) in reading.pressures {
                let mut pressure = u::TankPressure::default();
                pressure.value = pascal;
                pressure.tank_ref = Some(mix_id);
                waypoint.tank_pressures.push(pressure);
            }
            waypoint.temperature = reading.temperature;
            if instant == first {
                waypoint.dive_mode.clone_from(&divemode);
            }
            waypoint.gradient_factor = reading.gradient_factor.map(|value| {
                let mut factor = u::GradientFactor::default();
                factor.value = value;
                factor
            });
            waypoint.no_deco_time = reading.ndl;
            element.samples.push(waypoint);
        }

        let span = [
            &profile.depth,
            &profile.temperature,
            &profile.ndl,
            &profile.ppo2,
            &profile.cns,
            &profile.gradient_factor,
        ]
        .into_iter()
        .flatten()
        .map(|s| &s.times)
        .chain(profile.pressures.iter().map(|p| &p.times))
        .filter_map(|times| times.last().copied())
        .max()
        .unwrap_or(0);
        if profile.duration != span {
            let seconds = |ms: u64| in_seconds(i64::try_from(ms).unwrap_or(i64::MAX));
            self.note(
                &profile_at,
                format!("the profile's duration is {} s where its samples span {} s, and UDDF records no duration for a profile; a reader takes the span (spec §6.4)", seconds(profile.duration), seconds(span)),
                NoteKind::Dropped,
            );
        }
        numbered
    }

    fn events(
        &mut self,
        profile: &Profile,
        at: &str,
        mixes: &HashMap<u32, String>,
        readings: &mut BTreeMap<u64, Reading>,
    ) {
        for (index, event) in profile.events.iter().enumerate() {
            let event_at = format!("{at}/events/{index}");
            self.unmapped(&event_at, event, &["time", "type", "gas_number", "label"]);
            let instant = event.time;
            let when = in_seconds(i64::try_from(instant).unwrap_or(i64::MAX));
            let kind = event.event_type.as_ref();
            if kind == Some(&EventType::GasSwitch) {
                let Some(mix_id) = event.gas_number.and_then(|n| mixes.get(&n)) else {
                    self.note(
                        &event_at,
                        "UDDF's <switchmix> must name the gas switched to, and the event names none this dive has a cylinder for; the switch is dropped",
                        NoteKind::Dropped,
                    );
                    continue;
                };
                if readings.get(&instant).is_some_and(|r| r.switch.is_some()) {
                    self.note(
                        &event_at,
                        format!("a gas switch at {when} s shares its instant with an earlier one, and a waypoint carries one <switchmix>; the later switch is dropped"),
                        NoteKind::Dropped,
                    );
                    continue;
                }
                readings.entry(instant).or_default().switch = Some(mix_id.clone());
                continue;
            }
            let marker_type = kind.filter(|k| {
                matches!(
                    k,
                    EventType::DeepStop | EventType::SafetyStop | EventType::Bookmark
                )
            });
            let label = event.label.as_ref().filter(|l| !l.is_empty());
            let marker = marker_type.map(type_name).or_else(|| label.cloned());
            if let (Some(kind), Some(_)) = (marker_type, label) {
                self.note(
                    &event_at,
                    format!("UDDF's <setmarker> carries one string, and the event is a {} with a label; the type is written and the label is dropped", type_name(kind)),
                    NoteKind::Dropped,
                );
            } else if let (Some(kind), None, Some(_)) = (kind, marker_type, label) {
                self.note(
                    &event_at,
                    format!("UDDF's <setmarker> carries one string and has no keyword for a {}; the label is written and the type is dropped", type_name(kind)),
                    NoteKind::Dropped,
                );
            }
            let Some(marker) = marker else {
                let what = kind.map_or_else(
                    || "neither typed nor labelled".to_owned(),
                    |k| format!("an unlabelled {}", type_name(k)),
                );
                self.note(
                    &event_at,
                    format!("UDDF's <setmarker> carries only text, and the event is {what}; it is dropped rather than written as the word"),
                    NoteKind::Dropped,
                );
                continue;
            };
            if readings.get(&instant).is_some_and(|r| r.marker.is_some()) {
                self.note(
                    &event_at,
                    format!("an event at {when} s shares its instant with an earlier one, and a waypoint carries one <setmarker>; the later event is dropped"),
                    NoteKind::Dropped,
                );
                continue;
            }
            readings.entry(instant).or_default().marker = Some(marker);
        }
    }
}
