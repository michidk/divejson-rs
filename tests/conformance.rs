//! The vendored conformance corpus: every document must parse, validate, satisfy the
//! vendored JSON Schema and round-trip; every file in `fixtures/invalid/` must be rejected
//! the way its defect demands.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema() -> jsonschema::Validator {
    let text = fs::read_to_string(root().join("schema/1.0/divejson.schema.json")).unwrap();
    jsonschema::validator_for(&serde_json::from_str(&text).unwrap()).unwrap()
}

/// Every `.divejson` document under `fixtures/` outside `invalid/`: `valid/`, and the
/// expected outputs of the converter pairs, which are conforming documents too.
fn conforming_documents() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().unwrap() != "invalid" {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "divejson") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join("fixtures"), &mut out);
    out.sort();
    out
}

/// What §4 makes equivalent and JSON numbers make ambiguous: absent and empty arrays and
/// `extensions` objects, and `30` beside `30.0`.
fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, value)| match value {
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(members) => !(key.as_str() == "extensions" && members.is_empty()),
                    _ => true,
                })
                .map(|(key, value)| (key.clone(), normalize(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
        Value::Number(number) => Value::from(number.as_f64().unwrap()),
        other => other.clone(),
    }
}

#[test]
fn conforming_documents_parse_validate_and_round_trip() {
    let schema = schema();
    let documents = conforming_documents();
    assert!(
        documents.len() > 40,
        "corpus went missing: {}",
        documents.len()
    );
    for path in &documents {
        let name = path.strip_prefix(root()).unwrap().display();
        let text = fs::read_to_string(path).unwrap();
        let input: Value = serde_json::from_str(&text).unwrap();
        assert!(
            schema.is_valid(&input),
            "{name}: the vendored schema rejects it"
        );

        let doc = divejson::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let written = divejson::to_string_pretty(&doc).unwrap();
        let output: Value = serde_json::from_str(&written).unwrap();
        if let Err(error) = schema.validate(&output) {
            panic!("{name}: written output fails the schema: {error}");
        }
        assert_eq!(
            divejson::from_str(&written).unwrap(),
            doc,
            "{name}: re-read differs"
        );
        assert_eq!(
            normalize(&output),
            normalize(&input),
            "{name}: written JSON differs"
        );
    }
}

/// What rejects each invalid fixture: `None` for the typed parse (wrong shape, undefined
/// member, unknown value, null, duplicate member, malformed scalar), or the violation code.
const INVALID: &[(&str, Option<&str>)] = &[
    (
        "accommodation-dangling-reference",
        Some("dangling_reference"),
    ),
    ("address-without-country", None),
    ("agency-other-missing", Some("missing_member")),
    ("avg-depth-exceeds-max", Some("avg_depth_exceeds_max")),
    ("bad-version", None),
    ("bbox-missing-corner", None),
    ("bbox-south-exceeds-north", Some("south_exceeds_north")),
    ("bbox-without-position", Some("missing_member")),
    ("boat-name-empty", Some("length")),
    ("boat-name-too-long", Some("length")),
    ("channel-length-mismatch", Some("series_length_mismatch")),
    ("contact-position-incomplete", None),
    ("contact-roles-repeated", Some("duplicate_item")),
    (
        "course-agency-other-without-agency",
        Some("unexpected_member"),
    ),
    ("course-dates-reversed", Some("ends_before_start")),
    ("current-unknown", None),
    (
        "cylinder-gear-dangling-reference",
        Some("dangling_reference"),
    ),
    ("cylinder-gear-repeated", Some("duplicate_reference")),
    ("dangling-reference", Some("dangling_reference")),
    ("deco-model-gf-order", Some("gf_low_exceeds_high")),
    ("device-empty-member", Some("length")),
    ("dive-duration", None),
    ("dive-profile-outside-recording", None),
    ("dive-readout-outside-recording", None),
    ("diver-email-too-long", Some("length")),
    ("diver-name-too-long", Some("length")),
    ("diver-username", None),
    (
        "duplicate-file-uuid-across-recordings",
        Some("duplicate_uuid"),
    ),
    ("duplicate-file-uuid-portrait", Some("duplicate_uuid")),
    ("duplicate-json-member", None),
    ("duplicate-uuid", Some("duplicate_uuid")),
    ("emergency-contact-without-name", None),
    ("entry-type-unknown", None),
    ("event-without-type-or-label", Some("missing_member")),
    ("external-id-identifier-empty", Some("length")),
    ("external-id-identifier-too-long", Some("length")),
    ("external-id-openstreetmap-form", Some("pattern")),
    ("external-id-registry-not-a-key", Some("pattern")),
    ("external-id-wikidata-form", Some("pattern")),
    ("external-id-without-identifier", None),
    ("external-id-without-registry", None),
    ("external-id-worms-form", Some("pattern")),
    (
        "external-ids-repeated-with-extensions",
        Some("duplicate_external_id"),
    ),
    ("external-ids-repeated", Some("duplicate_external_id")),
    ("gear-dive-time-negative", None),
    ("gear-material-unknown", None),
    ("gear-type-unknown", None),
    ("gear-volume-not-positive", Some("range")),
    ("gear-working-pressure-too-high", Some("range")),
    ("instructor-dangling-reference", Some("dangling_reference")),
    ("insurance-without-provider", None),
    ("location-full-name", None),
    ("missing-format", None),
    ("naive-exported-at", Some("exported_at_without_offset")),
    ("non-increasing-samples", Some("times_not_increasing")),
    ("null-member", None),
    ("oxygen-helium-sum", Some("oxygen_helium_exceed_100")),
    ("person-dangling-reference", Some("dangling_reference")),
    ("person-referenced-twice", Some("duplicate_reference")),
    ("position-incomplete", None),
    ("pressure-order", Some("end_pressure_exceeds_start")),
    ("profile-duration-short", Some("duration_too_short")),
    ("rating-out-of-range", Some("range")),
    ("recording-without-content", Some("empty_recording")),
    ("schedule-interval-hours-not-positive", Some("range")),
    ("schedule-without-interval", Some("missing_member")),
    ("sighting-count-and-abundance", Some("unexpected_member")),
    ("sighting-dangling-reference", Some("dangling_reference")),
    ("sighting-species-repeated", Some("duplicate_reference")),
    ("site-altitude-too-high", Some("range")),
    ("site-altitude-too-low", Some("range")),
    ("site-bbox-south-exceeds-north", Some("south_exceeds_north")),
    ("site-depth-from-negative", Some("range")),
    ("site-depth-reversed", Some("depth_from_exceeds_to")),
    ("site-depth-to-negative", Some("range")),
    ("site-entry-type-unknown", None),
    ("site-entry-types-repeated", Some("duplicate_item")),
    ("site-other-name-empty", Some("length")),
    ("site-other-name-repeats-name", Some("duplicate_name")),
    ("site-other-name-too-long", Some("length")),
    ("site-other-names-repeated-by-case", Some("duplicate_name")),
    ("site-other-names-repeated", Some("duplicate_name")),
    ("site-tag-empty", Some("length")),
    ("site-tag-too-long", Some("length")),
    ("site-tags-repeated-by-case", Some("duplicate_tag")),
    ("site-tags-repeated", Some("duplicate_tag")),
    ("site-water-type-unknown", None),
    ("species-aphia-id", None),
    ("species-external-ids-empty", Some("missing_member")),
    (
        "species-external-ids-repeated-with-extensions",
        Some("duplicate_external_id"),
    ),
    ("species-no-identity", Some("missing_member")),
    ("species-two-worms-entries", Some("multiple_worms_entries")),
    ("species-wikidata-qid", None),
    ("tag-empty", Some("length")),
    ("tag-too-long", Some("length")),
    ("tags-repeated-by-case", Some("duplicate_tag")),
    ("tags-repeated-by-whitespace", Some("duplicate_tag")),
    ("tags-repeated", Some("duplicate_tag")),
    ("trailing-newline-datetime", None),
    ("trip-dates-reversed", Some("ends_before_start")),
    ("type-unknown", None),
    ("undefined-member", None),
    ("water-type-en13319", None),
    ("waves-unknown", None),
    ("weather-unknown", None),
];

#[test]
fn invalid_documents_are_rejected() {
    let dir = root().join("fixtures/invalid");
    let mut files: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|name| name.strip_suffix(".divejson").map(str::to_string))
        .collect();
    files.sort();
    let mut listed: Vec<&str> = INVALID.iter().map(|(name, _)| *name).collect();
    listed.sort_unstable();
    assert_eq!(
        files, listed,
        "every invalid fixture needs an expected outcome here"
    );
    for (name, expected) in INVALID {
        let result = divejson::from_slice(&fs::read(dir.join(format!("{name}.divejson"))).unwrap());
        match (result, expected) {
            (Err(divejson::Error::Json(_)), None) => {}
            (Err(divejson::Error::Invalid(violations)), Some(code)) => {
                assert!(
                    violations.iter().all(|v| v.code.as_str() == *code),
                    "{name}: expected only {code}, got {violations:?}"
                );
            }
            (other, _) => panic!("{name}: expected {expected:?}, got {other:?}"),
        }
    }
}

#[test]
fn writing_refuses_an_invalid_document() {
    let mut doc = divejson::from_str(
        r#"{"format":"divejson","version":"1.0","exported_at":"2026-04-17T11:49:23Z"}"#,
    )
    .unwrap();
    let mut dive = divejson::Dive::new(
        "0198a6f0-1111-7010-8000-000000000010".parse().unwrap(),
        "2026-04-17".parse::<divejson::Date>().unwrap(),
    );
    dive.max_depth = Some(10.0);
    dive.avg_depth = Some(12.0);
    doc.dives.push(dive);
    let err = divejson::to_string(&doc).unwrap_err();
    assert_eq!(err.violations()[0].code, divejson::Code::AvgDepthExceedsMax);
    assert_eq!(err.violations()[0].path, "/dives/0");
}
