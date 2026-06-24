# AGENTS.md

`tracing-subscriber` is the consumer-facing crate where telemetry pipelines are assembled — a base `Subscriber` (usually `Registry`) with a stack of composable `Layer`s providing fmt output, filtering, and span storage. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

The composition model is the core idea: a base `Subscriber` is wrapped by `Layer`s using `SubscriberExt::with(...)` (from the `prelude`), which produces a `Layered<L, S>` that is itself a `Subscriber`. Repeated `.with(...)` stacks layers; `tracing-core` still requires exactly one authoritative source of span IDs, which the base provides.

- `registry/` — `Registry` (`registry/sharded.rs`) is the canonical base `Subscriber`: a `sharded-slab`-backed store of per-span `Data` that layers read through the `LookupSpan` trait (`registry/mod.rs`). `LookupSpan::span(id)` returns a `SpanRef`, which walks parents via `Scope`/`ScopeFromRoot` and exposes a typed `Extensions`/`ExtensionsMut` map (`registry/extensions.rs`) so layers can attach their own per-span state. Layers that need stored spans bound `S: Subscriber + for<'a> LookupSpan<'a>`.
- `layer/` — defines the central `Layer<S>` trait (`layer/mod.rs`): observational hooks (`on_new_span`, `on_event`, `on_enter`/`on_exit`, `on_close`) plus `register_callsite`/`enabled` for interest, given a `Context<'a, S>` (`layer/context.rs`) that grants registry lookups. `Layered` (`layer/layered.rs`) is the runtime composition; `Identity` is the `Copy` no-op layer. `Layer` extends the hidden `layer::AsAny` helper and forwards component downcasts with safe `downcast_ref_by_id` references.
- `filter/` — two filtering tiers. **Global** filters are layers that short-circuit the whole stack; **per-layer** filters attach to one layer via `Layer::with_filter(f)`, yielding a `filter::Filtered<L, F, S>` (`filter/layer_filters/mod.rs`). Per-layer filtering works only on a registry: each `Filtered` gets a `FilterId` and records its verdict into a per-span `FilterMap` bitmap so sibling layers stay unaffected. Concrete filters: `EnvFilter` (`RUST_LOG`-style directives parsed in `filter/env/`, built via `Builder`), `Targets`, `LevelFilter`, and `filter_fn`/`FilterFn`/`DynFilterFn`.
- `fmt/` — `fmt::Layer<S, N, E, W>` (`fmt/fmt_layer.rs`) is the formatting layer, generic over field formatter `N: FormatFields` (`DefaultFields`), event formatter `E: FormatEvent` (`Format<Full>`; `Compact`/`Pretty`/`Json` in `fmt/format/`), and writer `W: MakeWriter` (`fmt/writer.rs`, default stdout). `fmt::Subscriber`/`FmtSubscriber` (`fmt/mod.rs`) bundles that layer over a `Registry` with an `EnvFilter`, built via `SubscriberBuilder` (`fmt()` / `fmt::init()`). Timestamps come from `FormatTime` impls in `fmt/time/` (`SystemTime`, `Uptime`, plus `time`/`chrono` crate formatters).
- `reload.rs` — `reload::Layer`/`Handle` wrap any layer to swap it at runtime (commonly to change a filter). `util.rs` adds `SubscriberInitExt` (`.init()`/`.try_init()`/`.set_default()`); under the `tracing-log` feature, `set_default()` initializes `LogTracer`. `field/` provides the `MakeVisitor`/`VisitFmt` field-visitor machinery used by formatters.

## Features

Feature gating cascades: `fmt ⇒ registry ⇒ std`, `ansi ⇒ fmt`, `registry ⇒ std`, `env-filter ⇒ std` (and pulls in `tracing`, `matchers`, `regex-automata`, `thread_local`). `json ⇒ tracing-serde + serde_json`. The `Layer` trait and `filter_fn` work under `no_std`+`alloc`, but `EnvFilter`, `fmt`, and `Registry` all require `std`. `valuable` is unstable (gated behind `--cfg tracing_unstable`, not just the Cargo feature). `regex` and `nu-ansi-term` features exist for back-compat / explicit ANSI opt-in.

## Testing

```bash
cargo nextest run --profile ci --all-features -p tracing-subscriber   # CI runs this crate with all features
```

The big integration suites are multi-file directories with a `main.rs` entry that self-gates: `tests/env_filter/` (`#![cfg(feature = "env-filter")]`, submodule `per_layer.rs` adds `#![cfg(feature = "registry")]`) and `tests/layer_filters/` (`#![cfg(feature = "registry")]`, submodules `boxed`/`targets`/`trees`/`vec`/`per_event`/etc.). Because the gates are `cfg`-based, running without the matching feature compiles them to empty — use `--all-features` (or the explicit feature) to actually exercise them. Most tests assert exact span/event output via `tracing-mock`'s `expect::{...}` matchers.

```bash
cargo bench -p tracing-subscriber --bench filter   # also: enter, filter_log, fmt (all harness = false / criterion)
```

Benches share `benches/support/mod.rs` (`MultithreadedBench`, a barrier-synchronized multi-thread driver); `enter` measures span enter/exit, `filter`/`filter_log` measure filter throughput single- vs multi-threaded, `fmt` measures span creation and event throughput.

## Gotchas

- Per-layer filtering is registry-only: `LookupSpan::register_filter` panics on subscribers that don't support it. When `registry`/`std` are off, `filter/mod.rs` swaps in `has_plf_stubs` so the PLF-detection hooks still compile.
- A `Layer`'s `enabled`/`register_callsite` interest is global to the stack; to scope filtering to a single layer use `with_filter` (per-layer `Filtered`), not the layer's own `enabled`. The `layer/mod.rs` rustdoc draws this Global-vs-Per-Layer distinction explicitly.
- `cargo hack` powerset is restricted to `fmt ansi json registry env-filter` for this crate (the full powerset is too large — root `AGENTS.md` explains the convention).
- `#![no_std]` crate: code outside `std`/`alloc`-gated modules must avoid `std`; the crate `extern crate alloc/std` only under the matching feature.
- Tests whose subject is filtering, layer ordering, or mock dispatch should prefer `tracing::subscriber::set_default` / `with_default` so the process-global `LogTracer` side effect does not enter their expected event stream. Keep explicit `SubscriberInitExt` + log bridge coverage in `tests/utils.rs`.
