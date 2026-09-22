# AGENTS.md

`tracing-serde` is the compatibility layer that serializes `tracing-core` types (events, spans, metadata, fields) through `serde`; it is the engine behind `tracing-subscriber`'s `json` formatter. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` — the sealed `AsSerde<'a>` extension trait adds `.as_serde()` to `Metadata`, `Event`, `Parent`, `span::{Attributes, Id, Record}`, `Level`, `Field`, and `FieldSet`, returning thin borrowed `Serialize` adapters. `SerializeEvent` and `SerializeAttributes` each serialize separate `metadata`, `parent`, and `fields` members. `SerializeParent` delegates directly to the core-owned `Root`, `Current`, or `Explicit(Id)` variant; serialization does not reconstruct parent state from boolean or optional-ID projections. Keeping recorded fields nested prevents user keys from replacing structural members.
- `SerdeMapVisitor<S: SerializeMap>` and `SerdeStructVisitor<S: SerializeStruct>` bridge `tracing_core::field::Visit` onto serde. Both use native integer, floating-point, boolean, string, and byte serialization, including `i128` and `u128`; byte fields use `Serializer::serialize_bytes`. Both retain the first field serialization error and skip later fields; `finish()` returns that native error. `SerdeMapVisitor` exposes `new()`/`finish()`/`take_serializer()` for downstream reuse.
- `fields.rs` — the sealed `AsMap` trait adds `.field_map()` returning `SerializeFieldMap<'a, T>` for `Event`, `Attributes`, and `Record`. Maps use an unknown entry count because declared fields can be absent or record no value; a metadata field count cannot stand in for the number of emitted entries.

## Features

- `valuable` (unstable) — requires building with `--cfg tracing_unstable` (its deps live under `[target.'cfg(tracing_unstable)'.dependencies]`). Enables the `record_value` impls on both visitors, serializing `valuable::Value`s via `valuable-serde`, and turns on `tracing-core/valuable`.

## Testing

- No `tests/` dir; coverage is via doctests in `lib.rs` and via dependents (chiefly `tracing-subscriber`'s `json` formatter, which imports `AsSerde`, `SerdeMapVisitor`, and `fields::AsMap`). Run `cargo test --doc -p tracing-serde`.
- Exercise the `valuable` path with `RUSTFLAGS="--cfg tracing_unstable" cargo test -p tracing-serde --features valuable`.

## Gotchas

- The `valuable` `record_value` methods are gated on `all(tracing_unstable, feature = "valuable")` — enabling the Cargo feature alone (without the `tracing_unstable` cfg) compiles them out silently.
- `AsSerde` and `AsMap` are sealed (private `sealed::Sealed` supertrait); downstream crates cannot implement them for new types.
