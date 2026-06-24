# AGENTS.md

`tracing-attributes` is the proc-macro crate (`proc-macro = true`) implementing the `#[instrument]` attribute, re-exported by `tracing` behind its `attributes` feature. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

Three modules, one public entry point:

- `lib.rs` — the `#[proc_macro_attribute] instrument` entry. It runs a two-pass strategy: `instrument_precise` parses the item as a full `syn::ItemFn` (rejecting `const fn` with a `compile_error!`, and detecting async-trait patterns), and on any parse error falls back to `instrument_speculative`, which parses a `MaybeItemFn` — a relaxed `ItemFn` whose body is kept as a raw `TokenStream` (so it can wrap functions whose bodies don't fully parse, e.g. unstable syntax). Both paths funnel into `expand::gen_function`.
- `attr.rs` — argument parsing. `InstrumentArgs` (the `Parse` impl driving the whole `#[instrument(...)]` arg list) collects `name`/`target`/`parent`/`follows_from`/`level`/`skip`/`skip_all`/`fields`/`err`/`ret`. Custom keywords live in `mod kw`. Supporting types: `Level` (string `"info"`, numeric `1`–`5`, or a `Path` like `Level::DEBUG`), `EventArgs` + `FormatMode` (`Debug`/`Display`) for `err(...)`/`ret(...)`, and `Fields`/`Field`/`FieldName`/`FieldKind` for the `fields(...)` DSL.
- `expand.rs` — codegen. `gen_function` → `gen_block` emit the span-creating wrapper; `AsyncInfo::from_fn` / `gen_async` rewrite async-trait-style bodies (`async-trait <= 0.1.43`'s inner `async fn` + `Box::pin`, and `>= 0.1.44`'s `Box::pin(async move {...})`) so the *inner future* is instrumented rather than the allocating wrapper. Several generated fragments deliberately include narrow `#[allow(clippy::...)]` attributes because the macro must compile under downstream lint settings.

Non-obvious: unrecognized `#[instrument]` args are not hard errors. `InstrumentArgs::warnings()` accumulates them and emits a fake `#[deprecated]` const so they surface as warnings (backwards-compat hack until `proc_macro::Diagnostic` stabilizes).

## Features

- `async-await` — a no-op kept only for backwards compatibility (`# This feature flag is no longer necessary`). Don't add logic behind it.

## Testing & benches

- Behavioral tests live in `tests/` and assert exact span/event/field output via `tracing-mock` (workspace dev-dep), with `tracing-test` supplying `PollN`/`block_on_future` for the async cases: `async_fn`, `err`, `ret`, `fields`, `levels`, `parents`, `follows_from`, `names`, `targets`, `destructuring`, `dead_code`, `instrument`. Run one with plain cargo: `cargo test -p tracing-attributes --test instrument`.
- UI / compile-fail tests: `tests/ui.rs` drives `trybuild` over `tests/ui/pass/*.rs` and `tests/ui/fail/*.rs` (current fail fixtures: `async_instrument`, `const_instrument`, `unused_instrumented_fn`; pass: `type_shadowing`). Both test fns are gated `#[rustversion::stable]`, so they are skipped on beta/nightly — `.stderr` fixtures are stable-pinned by design. Editing any `tests/ui/**/*.stderr` requires the forked `strict-trybuild` git dep to resolve (see root `AGENTS.md`).
- No benches in this crate.

## Gotchas

- The two-pass parse means a change to `gen_function`/`gen_block` must work for *both* a real `syn::Block` and a raw-`TokenStream` body; don't assume the body is parseable AST.
- A `fields(...)` entry whose name matches a non-skipped argument implicitly skips that argument (to avoid double-recording) — unless the field name is a constant `{EXPR}` form, in which case dedup would cost runtime and you must `skip` it explicitly.
- A bare `fields(foo)` (no value) emits `::tracing::field::Empty`, not a local-variable shorthand — this is a deliberately frozen breaking-change footgun noted in `attr.rs`.
- Test fixtures inherit the workspace lint policy too. New integration-test files need crate-level docs, intentionally discarded results should be named, and unsafe fixtures need a local `#[allow(unsafe_code, reason = "...")]` explaining the macro behavior being exercised.
