# divejson

The Rust implementation of [DiveJSON](https://divejson.org), the open dive-log interchange
format: a typed model of a DiveJSON 1.0 document, a strict reader, a writer that only emits
conforming documents, and a validator for every rule of the specification.

```sh
cargo add divejson
```

## Reading and writing

```rust
let text = std::fs::read_to_string("logbook.divejson")?;
let mut doc = divejson::from_str(&text)?; // parses and validates

for dive in &doc.dives {
    println!("{} max {:?} m", dive.started_at, dive.max_depth);
}

let mut dive = divejson::Dive::new(
    "0198a6f0-1111-7010-8000-000000000010".parse()?,
    "2026-04-17T11:49:23+02:00".parse::<divejson::DateTime>()?,
);
dive.max_depth = Some(18.4); // meters
doc.dives.push(dive);

let out = divejson::to_string_pretty(&doc)?; // validates first
```

`from_str`, `from_slice` and `from_reader` read a document; `to_string`, `to_string_pretty`,
`to_vec` and `to_writer` write one. Units follow the specification (§5.1) and are documented
on each field: meters, degrees Celsius, bar, liters, percent and seconds; profile channels
are integers on a millisecond axis (depth in centimeters, temperature in hundredths of a
degree, pressure in tenths of a bar).

Date-times keep the text they were written as, so an offset, a `Z` and the fractional
digits survive a round trip, and a local date-time without an offset stays a wall clock.
A dive's `started_at` may be a date alone. Absent members are `None` or an empty `Vec`;
`null` is never read or written.

## Validation

Reading is strict and happens in two steps. The parse rejects anything that does not have
the schema's shape: malformed JSON, a duplicate member name, `null`, an undefined member
outside `extensions`, an unknown vocabulary value, a missing required member, a malformed
uuid, date or date-time (`Error::Json`). `Document::validate` then checks the remaining
schema constraints (lengths, ranges, patterns, conditional members) and every requirement
§3 of the specification lists as living outside the schema: uuid uniqueness and referential
closure, cross-member arithmetic, profile-series integrity, recording content, the offset on
`exported_at`, the gradient-factor order, references and registry entries listed once, and
tags and site names compared trimmed and case-folded. Each `Violation` carries a stable
`Code`, a JSON Pointer path and a message (`Error::Invalid`).

The JSON Schema's `format: email` and `format: uri` annotations are not asserted, which §3
allows.

This reader implements 1.0 strictly. It does not apply §5.6's tolerance for a newer minor
version (ignoring unknown members, dropping unknown vocabulary values); a document declaring
any version other than `1.0` is rejected.

## Feature flags

| feature | default | what |
| --- | --- | --- |
| `std` | yes | `from_reader`, `to_writer` and `std::error::Error`. Without it the crate is `no_std` with `alloc`. |
| `uddf` | no | `divejson::uddf`: read UDDF into DiveJSON and write DiveJSON as UDDF, through the [`uddf`](https://github.com/michidk/uddf-rs) crate. Turns on `std`. Enable with `cargo add divejson --features uddf`. |

The crate builds for `wasm32-unknown-unknown`, with and without `std`, and with `uddf`.

## UDDF

With the `uddf` feature, `divejson::uddf::read` converts a UDDF file into a DiveJSON
document and `divejson::uddf::write` writes one back out, each returning a report beside its
output. They follow the specification's [`docs/uddf-mapping.md`](docs/uddf-mapping.md) and
[`docs/uddf-writing.md`](docs/uddf-writing.md).

```rust
let text = std::fs::read_to_string("logbook.uddf")?;
let exported_at = "2026-09-05T00:00:00+00:00".parse()?; // the moment of conversion
let (doc, report) = divejson::uddf::read(&text, exported_at)?;
for note in &report {
    println!("{:?} {}: {}", note.kind, note.at, note.message); // e.g. Absent dive/0: …
}

let (xml, report) = divejson::uddf::write(&doc)?; // UDDF 3.2.2
```

The reader is lenient: a malformed value is read as absent and reported, and only malformed
XML, a `<!DOCTYPE>` or a root other than `<uddf>` is an error (`Error::Uddf`). Records
without a UUID in their source id get a UUIDv5 under `divejson::uddf::ID_NAMESPACE`, so
converting the same file twice gives the same document. Each report entry carries its kind
(`Absent`, `Inferred`, `Resolved`, `Dropped`), where it is (a path into the source when
reading, into the document when writing), the DiveJSON member it concerns, and, when
reading, a stable code such as `start_time_zone_missing`.

## Conformance

The repository vendors the specification's `schema/`, `fixtures/` and `docs/` at the commit
in [`SPEC_REF`](SPEC_REF). The tests:

- parse, validate and round-trip every conforming document in `fixtures/` (the `valid/`
  documents and the expected outputs of every converter pair), check each against the
  vendored JSON Schema before and after writing, and compare the written JSON with the input;
- reject every document in `fixtures/invalid/`, each with the outcome recorded for it: a
  parse error or one specific violation code;
- with `uddf`, read every `fixtures/uddf/X.uddf` to `X.divejson` (compared as JSON, without
  `exported_at` and `generator`) and write every `fixtures/write/uddf/X.divejson` to `X.uddf`
  (compared as canonical XML, without `<generator>`).

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. The vendored specification files are MIT
licensed by the DiveJSON authors.
