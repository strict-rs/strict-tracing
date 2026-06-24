# AGENTS.md

`tracing-core` is the foundation crate of the workspace: it defines the primitives every other crate builds on (`Subscriber`, `Dispatch`, `Metadata`/`Callsite`, `Field`/`ValueSet`, span `Id`, `Event`) plus the global callsite registry and per-thread/global dispatcher. It is `no_std`-capable and intentionally the most stability-sensitive crate, since external subscriber authors depend on it directly. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

- `subscriber.rs` — the `Subscriber` trait, the contract every collector implements (`new_span`/`record`/`event`/`enter`/`exit`/`enabled`/`register_callsite`/`event_enabled`/`clone_span`/`try_close`). Many methods have default impls; `register_callsite` defaults to delegating to `enabled`. Also defines `Interest` (`always`/`sometimes`/`never`) and `NoSubscriber` (the `Copy` no-op default).
- `dispatcher.rs` — `Dispatch`, a cloneable type-erased `Arc<dyn Subscriber>`, plus the machinery to install one: `with_default` (thread-local, scoped, **std-only**), `set_global_default` (process-wide, once), and `get_default`. `WeakDispatch`/`Dispatch::downgrade` exist so a `Subscriber` can hold a back-reference without a refcount cycle.
- `callsite.rs` — the `Callsite` trait, `Identifier`, `DefaultCallsite` (the ready-made impl macros generate), and the **global callsite registry**. Each callsite caches a combined `Interest` so per-event filtering avoids calling `enabled`; `rebuild_interest_cache` invalidates it (also triggered automatically when a `Dispatch` is created/dropped). `dispatchers::Dispatchers` tracks active subscribers under `sync::Mutex`.
- `metadata.rs` — `Metadata` (static name/target/level/fields/file/line/module/`Kind`), the `Level`/`LevelFilter` ordering types, and `Kind` (bit-flag consts `SPAN`/`EVENT`/`HINT`).
- `field.rs` — `Field`/`FieldSet` (keys are array indices into a callsite's field list), `Value`/`ValueSet`, the `Visit` visitor trait, and `Empty`. Optionally bridges to `valuable` under `--cfg tracing_unstable`.
- `event.rs`/`span.rs` — `Event` and span `Id` (`NonZeroU64`), `Attributes`/`Record`. `parent.rs` is the internal `Parent` enum (`Root`/`Current`/`Explicit(Id)`) carried by `Attributes`/`Event`.
- `lib.rs` exports the `metadata!` and `identify_callsite!` constructor macros and inlines the central types at the crate root.

Data flow: instrumentation builds a static `Callsite`+`Metadata`; first use registers it and caches `Interest`; if enabled, it constructs `Attributes`/`Event` and hands them to the current `Dispatch`'s `Subscriber`.

## Features

- `std` (default) — pulls in `std`; without it the crate is `no_std` but still **requires `liballoc`** (`extern crate alloc`). With `std` off, `with_default`/thread-local dispatch is unavailable (use `set_global_default`), and the external workspace `spin` dependency plus `sync.rs` supply the spinlock-backed `Mutex` shape expected by the callsite registry. The old vendored `src/spin/` module is gone.
- `valuable` — unstable, gated behind `--cfg tracing_unstable` (a `cfg`, not a plain Cargo feature); the dependency lives under `[target.'cfg(tracing_unstable)'.dependencies]`.
- `once_cell` — vestigial no-op feature kept for back-compat (a former implicit optional-dep feature); do not build new functionality on it.

## Testing

- `cargo nextest run -p tracing-core` for the bulk; doctests (heavy here) via `cargo test --doc -p tracing-core`.
- The `std`-off build is its own CI step: `cargo test --no-default-features -p tracing-core`. `tests/dispatch.rs` is `#![cfg(feature = "std")]` (scoped/thread-local dispatch).
- `tests/global_dispatch.rs` and `tests/local_dispatch_before_init.rs` are **separate test binaries on purpose**: `set_global_default` can succeed only once per process, so each global-dispatch scenario needs its own process. Don't merge them into `dispatch.rs`. `tests/common/mod.rs` provides the shared `TestSubscriberA`/`TestSubscriberB` no-op subscribers.

## Gotchas

- This is the workspace's stability anchor: changing the `Subscriber`/`Callsite` trait surface or `Metadata`/`Field` layout breaks every downstream crate and external implementors. Treat additions as default-method/additive only unless a break is intended.
- `Subscriber` extends the hidden `subscriber::AsAny` helper and exposes `downcast_ref_by_id` as the object-safe component downcast hook. Composition wrappers should forward safe `&dyn Any` references; do not add raw-pointer downcast paths.
- `lib.rs` enables crate-level warnings and the manifest inherits `[lints] workspace = true`: every new public item needs docs, `Debug`/visibility need to satisfy the workspace lint policy, and intentionally ignored return values should use named `_foo` bindings rather than bare `let _ = ...`.
