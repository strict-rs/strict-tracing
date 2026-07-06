# AGENTS.md

`tracing-attributes-internal` is the normal (non-proc-macro) library holding all parsing and expansion logic behind the `tracing-attributes` facade; the facade's `#[proc_macro_attribute] instrument` delegates here. `publish = false` — its only consumer is the sibling shim, which is why the crate can expose a genuinely public module tree without that tree being a semver commitment. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

Three modules under a genuinely public tree (`pub mod attr;`, `pub mod entry;`, `pub mod expand;`, with `pub use crate::entry::instrument` as the primary surface). Everything is `proc_macro2` types — this crate never links `proc_macro` (only proc-macro crates can), which is what lets the code be an ordinary, unit-testable library.

- `entry.rs` — the pipeline `instrument(args, item_tokens) -> TokenStream` plus the item-token model `MaybeItemFn`/`MaybeItemFnRef`. Two-pass strategy: `instrument_precise` parses the item as a full `syn::ItemFn` (rejecting `const fn` with a `compile_error!`, and detecting async-trait patterns); on any parse error it falls back to `instrument_speculative`, which parses a `MaybeItemFn` — a relaxed `ItemFn` whose body is kept as a raw `TokenStream` (so it can wrap functions whose bodies don't fully parse, e.g. unstable syntax). Both paths funnel into `expand::gen_function`. Parsing goes through `syn::parse2` + `into_compile_error()` (the `parse_macro_input!` family is proc_macro-only and must not reappear here).
- `attr.rs` — argument parsing. `InstrumentArgs` (the `Parse` impl driving the whole `#[instrument(...)]` arg list) collects `name`/`target`/`parent`/`follows_from`/`level`/`skip`/`skip_all`/`fields`/`err`/`ret`, with each argument's duplicate guard testing its own field. Custom keywords live in `mod kw`. Supporting types: `Level` (string `"info"`, numeric `1`–`5`, or a `Path` like `Level::DEBUG`), `EventArgs` + `FormatMode` (`Debug`/`Display`) for `err(...)`/`ret(...)`, and `Fields`/`Field`/`FieldName`/`FieldKind` for the `fields(...)` DSL.
- `expand.rs` — codegen. `gen_function` → `gen_block` emit the span-creating wrapper; `AsyncInfo::from_fn` / `gen_async` rewrite async-trait-style bodies (`async-trait <= 0.1.43`'s inner `async fn` + `Box::pin`, and `>= 0.1.44`'s `Box::pin(async move {...})`) so the *inner future* is instrumented rather than the allocating wrapper. `gen_async` returns `proc_macro2::TokenStream`. Several generated fragments deliberately include narrow `#[allow(clippy::...)]` attributes in the *emitted* tokens because the macro output must compile under downstream lint settings — those are output text, not source-level suppressions.

Non-obvious: unrecognized `#[instrument]` args are not hard errors. `InstrumentArgs::warnings()` accumulates them and emits a fake `#[deprecated]` const so they surface as warnings (backwards-compat hack until `proc_macro::Diagnostic` stabilizes).

## Testing

The argument-parsing unit tests (including the dual-polarity duplicate-guard cases: every argument's duplicate rejected with its exact message; valid combinations like `target` + `parent` accepted) live in `attr.rs` as `#[cfg(test)] mod tests`, panic-free via the `strict-test-support` `ensure*` vocabulary. Behavioral and UI/compile-fail tests for the macro surface stay in the facade crate's `tests/` — if a change here alters observable macro behavior, its test belongs there.

## Gotchas

- The two-pass parse means a change to `gen_function`/`gen_block` must work for *both* a real `syn::Block` and a raw-`TokenStream` body; don't assume the body is parseable AST.
- A `fields(...)` entry whose name matches a non-skipped argument implicitly skips that argument (to avoid double-recording) — unless the field name is a constant `{EXPR}` form, in which case dedup would cost runtime and you must `skip` it explicitly.
- A bare `fields(foo)` (no value) emits `::tracing::field::Empty`, not a local-variable shorthand — this is a deliberately frozen breaking-change footgun noted in `attr.rs`.
- Visibility here encodes the facade contract: items the shim or a sibling module consumes are genuinely `pub`; module-local helpers stay private. Never reintroduce `pub(crate)` — the `unreachable_pub` + `redundant_pub_crate` deny pair is what forced this crate into existence.
