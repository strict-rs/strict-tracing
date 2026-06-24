# AGENTS.md

`tracing-serde` is the compatibility layer that serializes `tracing-core` types (events, spans, metadata, fields) through `serde`; it is the engine behind `tracing-subscriber`'s `json` formatter. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `lib.rs` — the `AsSerde<'a>` extension trait (sealed) adds `.as_serde()` to `Metadata`, `Event`, `span::{Attributes, Id, Record}`, `Level`, `Field`, and `FieldSet`, each returning a thin `Serialize` newtype wrapper: `SerializeMetadata`, `SerializeEvent`, `SerializeAttributes`, `SerializeId`, `SerializeRecord`, `SerializeLevel`, `SerializeField`, `SerializeFieldSet`. Because `tracing` values expose their fields only through the visitor pattern, two adapters bridge `tracing_core::field::Visit` onto serde: `SerdeMapVisitor<S: SerializeMap>` and `SerdeStructVisitor<S: SerializeStruct>`. Both short-circuit: once a `record_*` call errors, `state` holds it and later fields are skipped; `finish()` returns that error. `SerdeMapVisitor` is `pub` with `new()`/`finish()`/`take_serializer()` for downstream reuse.
- `fields.rs` — the `AsMap` trait (also sealed) adds `.field_map()` returning `SerializeFieldMap<'a, T>`, which serializes just an item's fields as a serde map (for `Event`/`Attributes`/`Record`), as opposed to the whole metadata struct.

## Features

- `valuable` (unstable) — requires building with `--cfg tracing_unstable` (its deps live under `[target.'cfg(tracing_unstable)'.dependencies]`). Enables the `record_value` impls on both visitors, serializing `valuable::Value`s via `valuable-serde`, and turns on `tracing-core/valuable`.

## Testing

- No `tests/` dir; coverage is via doctests in `lib.rs` and via dependents (chiefly `tracing-subscriber`'s `json` formatter, which imports `AsSerde`, `SerdeMapVisitor`, and `fields::AsMap`). Run `cargo test --doc -p tracing-serde`.
- Exercise the `valuable` path with `RUSTFLAGS="--cfg tracing_unstable" cargo test -p tracing-serde --features valuable`.

## Gotchas

- The `valuable` `record_value` methods are gated on `all(tracing_unstable, feature = "valuable")` — enabling the Cargo feature alone (without the `tracing_unstable` cfg) compiles them out silently.
- `AsSerde` and `AsMap` are sealed (private `sealed::Sealed` supertrait); downstream crates cannot implement them for new types.
