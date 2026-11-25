//! The UDDF converter pairs: every `fixtures/uddf/X.uddf` reads to `X.divejson`, and every
//! `fixtures/write/uddf/X.divejson` writes to `X.uddf`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use divejson::uddf::{self, NoteKind};
use serde_json::Value;

fn fixtures(dir: &str, suffix: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(dir);
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == suffix))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no {suffix} pairs");
    paths
}

fn exported_at() -> divejson::DateTime {
    "2026-09-05T00:00:00+00:00".parse().unwrap()
}

/// A document reduced to what a reader pair compares: everything but `exported_at` and
/// `generator`, with absent and empty arrays equal and `30` equal to `30.0`.
fn compared(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, value)| {
                    !matches!(key.as_str(), "exported_at" | "generator")
                        && !matches!(value, Value::Array(items) if items.is_empty())
                })
                .map(|(key, value)| (key.clone(), compared(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(compared).collect()),
        Value::Number(number) => Value::from(number.as_f64().unwrap()),
        other => other.clone(),
    }
}

#[test]
fn every_reader_pair_reads_to_its_document() {
    let mut failures = Vec::new();
    let paths = fixtures("uddf", "uddf");
    for path in &paths {
        let text = fs::read_to_string(path).unwrap();
        let (doc, report) = match uddf::read(&text, exported_at()) {
            Ok(read) => read,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        assert!(
            report.iter().all(|note| note.kind != NoteKind::Inferred),
            "this reader infers nothing"
        );
        let produced = serde_json::to_value(&doc).unwrap();
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(path.with_extension("divejson")).unwrap())
                .unwrap();
        if compared(&produced) != compared(&expected) {
            failures.push(format!(
                "{}:\nproduced {}\nexpected {}",
                path.display(),
                serde_json::to_string_pretty(&compared(&produced)).unwrap(),
                serde_json::to_string_pretty(&compared(&expected)).unwrap()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} reader pairs differ:\n{}",
        failures.len(),
        paths.len(),
        failures.join("\n\n")
    );
}

/// A UDDF document reduced to what a writer pair compares: element names, attributes
/// (namespace declarations aside) and trimmed text, in document order, without
/// `<generator>` — the canonical-XML comparison `docs/uddf-writing.md` describes.
fn canonical(text: &str) -> String {
    fn walk(node: roxmltree::Node<'_, '_>, depth: usize, out: &mut String) {
        if node.tag_name().name() == "generator" && depth == 1 {
            return;
        }
        let mut attributes: Vec<String> = node
            .attributes()
            .map(|a| format!("{}={:?}", a.name(), a.value()))
            .collect();
        attributes.sort();
        let text: String = node
            .children()
            .filter(roxmltree::Node::is_text)
            .filter_map(|n| n.text())
            .collect();
        let _ = writeln!(
            out,
            "{}<{} {}>{}",
            "  ".repeat(depth),
            node.tag_name().name(),
            attributes.join(" "),
            text.trim()
        );
        for child in node.children().filter(roxmltree::Node::is_element) {
            walk(child, depth.saturating_add(1), out);
        }
    }
    let doc = roxmltree::Document::parse(text).unwrap();
    let mut out = String::new();
    walk(doc.root_element(), 0, &mut out);
    out
}

#[test]
fn every_writer_pair_writes_its_file() {
    let mut failures = Vec::new();
    let paths = fixtures("write/uddf", "divejson");
    for path in &paths {
        let doc = divejson::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        let (written, _report) = uddf::write(&doc).unwrap();
        let expected = fs::read_to_string(path.with_extension("uddf")).unwrap();
        let (produced, expected) = (canonical(&written), canonical(&expected));
        if produced != expected {
            let first = produced
                .lines()
                .zip(expected.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let around = |text: &str| {
                text.lines()
                    .skip(first.saturating_sub(3))
                    .take(8)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            failures.push(format!(
                "{} differs at line {first}:\nproduced\n{}\nexpected\n{}",
                path.display(),
                around(&produced),
                around(&expected)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} writer pairs differ:\n{}",
        failures.len(),
        paths.len(),
        failures.join("\n\n")
    );
}

#[test]
fn a_written_pair_reads_back_through_the_reader() {
    for path in fixtures("write/uddf", "divejson") {
        let doc = divejson::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let (written, _) = uddf::write(&doc).unwrap();
        let (again, _) = uddf::read(&written, exported_at()).unwrap();
        assert_eq!(again.dives.len(), doc.dives.len(), "{}", path.display());
        for (one, other) in again.dives.iter().zip(&doc.dives) {
            assert_eq!(one.uuid, other.uuid, "{}", path.display());
            assert_eq!(one.started_at, other.started_at, "{}", path.display());
            assert_eq!(one.max_depth, other.max_depth, "{}", path.display());
        }
    }
}

#[test]
fn identity_is_derived_and_embedded_uuids_are_reused() {
    let text = r#"<uddf><diver><owner id="owner"><personal><firstname>Sam</firstname></personal></owner></diver>
      <profiledata><repetitiongroup id="g"><dive id="dive-0198A6F0-5555-7001-8000-000000000001">
        <informationbeforedive><datetime>2026-04-19T08:12</datetime></informationbeforedive>
      </dive><dive id="g"><informationbeforedive><datetime>2026-04-20</datetime></informationbeforedive></dive>
      </repetitiongroup></profiledata></uddf>"#;
    let (doc, report) = uddf::read(text, exported_at()).unwrap();
    assert_eq!(
        doc.diver.unwrap().uuid.unwrap().to_string(),
        "d8b9bbd3-c66e-52c1-b8e8-d908f921af7e"
    );
    assert_eq!(
        doc.dives[0].uuid.to_string(),
        "0198a6f0-5555-7001-8000-000000000001"
    );
    assert_eq!(doc.dives[0].started_at.to_string(), "2026-04-19T08:12:00");
    assert_eq!(doc.dives[1].started_at.to_string(), "2026-04-20");
    let codes: Vec<_> = report.iter().filter_map(|note| note.code).collect();
    assert!(codes.contains(&"start_time_zone_missing"));
    assert!(codes.contains(&"start_time_of_day_missing"));
}

#[test]
fn a_doctype_is_refused() {
    let text = r#"<!DOCTYPE uddf [<!ENTITY x "y">]><uddf/>"#;
    assert!(matches!(
        uddf::read(text, exported_at()),
        Err(divejson::Error::Uddf(_))
    ));
}
