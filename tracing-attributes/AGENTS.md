# AGENTS.md

`tracing-attributes` is the proc-macro facade crate (`proc-macro = true`) exposing the `#[instrument]` attribute, re-exported by `tracing` behind its `attributes` feature. All parsing and expansion logic lives in the sibling `tracing-attributes-internal` crate; this crate is a thin delegating shim. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

Facade/internal split, prescribed by the workspace lint policy: rustc lets a `proc-macro = true` crate export only `#[proc_macro]` items, so internal modules here can be neither genuinely `pub` (`unreachable_pub`) nor `pub(crate)` (`clippy::redundant_pub_crate`). The logic therefore lives in the normal library crate `tracing-attributes-internal` as a genuinely public module tree, and this crate keeps a single file:

- `lib.rs` — the published crate docs plus the `#[proc_macro_attribute] instrument` entry point, delegating as `tracing_attributes_internal::instrument(args.into(), item_tokens.into()).into()`. The `proc_macro` ↔ `proc_macro2` `TokenStream` conversions come from proc-macro2's `From` impls (reachable through the internal crate), so the shim's only dependency is `tracing-attributes-internal` — do not reintroduce direct `proc-macro2`/`syn`/`quote` dependencies here; unused ones fail the deadcode gate.

See `tracing-attributes-internal/AGENTS.md` for the parsing/expansion internals (the `entry`/`attr`/`expand` modules, the two-pass parse strategy, and the `fields(...)` DSL). The external contract — macro name, signature, and rustdoc — is owned by this crate and must stay byte-compatible when the internals move.

## Features

- `async-await` — a no-op kept only for backwards compatibility (`# This feature flag is no longer necessary`). Don't add logic behind it.

## Testing & benches

- Behavioral tests live in `tests/` and assert exact span/event/field output via `tracing-mock` (workspace dev-dep), with `tracing-test` supplying `PollN`/`block_on_future` for the async cases: `async_fn`, `err`, `ret`, `fields`, `levels`, `parents`, `follows_from`, `names`, `targets`, `destructuring`, `dead_code`, `instrument`. Run one with plain cargo: `cargo test -p tracing-attributes --test instrument`. They exercise the macro surface, so they stay in this crate; the argument-parsing unit tests (duplicate-guard dual-polarity cases) live in `tracing-attributes-internal` next to the code they cover.
- UI / compile-fail tests: `tests/ui.rs` drives `trybuild` over `tests/ui/pass/*.rs` and `tests/ui/fail/*.rs` (current fail fixtures: `async_instrument`, `const_instrument`, `unused_instrumented_fn`; pass: `type_shadowing`). Both test fns are gated `#[rustversion::stable]`, so they are skipped on beta/nightly — `.stderr` fixtures are stable-pinned by design. Editing any `tests/ui/**/*.stderr` requires the forked `strict-trybuild` git dep to resolve (see root `AGENTS.md`).
- No benches in this crate.

## Gotchas

- The two-pass parse means a change to `gen_function`/`gen_block` must work for *both* a real `syn::Block` and a raw-`TokenStream` body; don't assume the body is parseable AST.
- A `fields(...)` entry whose name matches a non-skipped argument implicitly skips that argument (to avoid double-recording) — unless the field name is a constant `{EXPR}` form, in which case dedup would cost runtime and you must `skip` it explicitly.
- A bare `fields(foo)` (no value) emits `::tracing::field::Empty`, not a local-variable shorthand — this is a deliberately frozen breaking-change footgun noted in `attr.rs`.
- Test fixtures inherit the workspace lint policy too. New integration-test files need crate-level docs, and intentionally discarded results should be named. Unsafe macro coverage belongs in `tests/ui/pass/*.rs` trybuild fixtures, not in compiled integration-test crates.
