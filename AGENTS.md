# divejson-rs

The Rust implementation of DiveJSON. The format itself is decided in
[divejson/divejson](https://github.com/divejson/divejson); read its `CONTRIBUTING.md`
("How a change lands: two repositories, two orders") before changing what validates.

## Ownership

- `schema/`, `fixtures/` and `docs/` are vendored from divejson/divejson at the commit in
  `SPEC_REF`. Never edit a file the specification owns. This repository may carry extra
  files there ahead of the specification (a new converter's pairs and mapping document).
- A change to the schema, a rule or an existing fixture lands in the specification first.
  Then re-vendor and move `SPEC_REF` in one step:

  ```bash
  git -C ../divejson fetch origin
  git -C ../divejson rev-parse origin/main > SPEC_REF
  rm -rf schema fixtures docs
  git -C ../divejson archive "$(cat SPEC_REF)" schema fixtures docs | tar -x
  ```

  Removing the directories first drops files the specification deleted; put back any file
  this repository carries ahead of it. Then update `INVALID` in `tests/conformance.rs` for
  new or changed invalid fixtures, and the model and validator for schema changes.

  CI checks byte equality with the pinned commit and that the pin is on the
  specification's `main`.

## Code

- `src/document.rs`, `dive.rs`, `recording.rs`, `records.rs`: the typed model, one struct per
  spec object, `#[non_exhaustive]` with `new(required…)` constructors. Absent is `None` or an
  empty `Vec`; `deny_unknown_fields` everywhere except `extensions`.
- `src/values.rs`: uuid, date and date-time types with the exact grammar of §5.2/§5.3.
- `src/read.rs`: rejects duplicate member names and `null` before the typed parse.
- `src/validate.rs`: every constraint the types do not carry. Violation codes are stable;
  add a new `Code` rather than reusing one for a different rule.
- Converters go in their own module behind a feature, using only the public model, and their
  pairs in `fixtures/<format>/`.
- `src/uddf/` (feature `uddf`, implies `std`): `read.rs` is UDDF to DiveJSON after
  `docs/uddf-mapping.md`, `write.rs` DiveJSON to UDDF after `docs/uddf-writing.md`; `mod.rs`
  holds the report types (`Note`, `NoteKind`, `Report`) and the shared unit helpers. The
  divejson-py reference implementation (`divejson/uddf.py`, `uddf_write.py`) is MIT and may be
  followed. XML parsing and writing uses the published `uddf` crate; a field its model
  lacks is added there first.
  `tests/uddf.rs` runs every reader and writer pair.
- Strict lints: no `unwrap`, `expect`, `panic` or indexing in library code. Keep the crate
  `no_std` + `alloc` without the `std` feature; put `std`-only code behind it.

## Checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test   # without uddf
cargo clippy --target wasm32-unknown-unknown --all-features -- -D warnings
cargo clippy --target wasm32-unknown-unknown --no-default-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check
```

Do not set `CARGO_TARGET_DIR` or pass `--target-dir`. Commits and pull request titles follow
Conventional Commits.
