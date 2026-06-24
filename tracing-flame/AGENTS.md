# AGENTS.md

`tracing-flame` is the flamegraph/flamechart layer: it records span enter/exit timings as "folded stack" text that `inferno` turns into an SVG. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Architecture

The pipeline is `FlameLayer` -> folded-stack text -> `inferno-flamegraph` (an external CLI; see the `inferno-flame` example under `examples/`).

- `lib.rs` defines `FlameLayer<S, W>`, a `tracing_subscriber::Layer` holding `out: Arc<Mutex<W>>` (any `W: io::Write`), a private `Config`, and `PhantomData<S>`. `on_enter`/`on_exit` build one folded-stack line per event — `thread-name;root::span;child::span <nanos>` — by walking `span.scope().from_root()` and writing it to `out`. The free `write` fn formats each frame, optionally prefixing `module_path` and suffixing `file:line` per `Config`.
- Sample counts are **not** samples: the trailing number is nanoseconds since the previous event on the same thread, tracked via the `LAST_EVENT` thread-local relative to a process-wide `START: Lazy<Instant>`.
- `FlushGuard<W>` (`#[must_use]`) holds a clone of `out` and flushes it on `Drop` (or via `.flush()`). It exists because a global subscriber's layers are never dropped at exit, so a buffered `W` would otherwise lose tail data. `FlameLayer::with_file(path)` is the convenience constructor returning `(FlameLayer<S, BufWriter<File>>, FlushGuard<BufWriter<File>>)`; `flush_on_drop()` mints a guard for an arbitrary writer.
- `error.rs` defines the opaque `Error(Kind)` (`Kind::CreateFile`/`FlushFile`) returned by `with_file`/`flush`; `Error::report()` prints the `source()` chain to stderr (used by the `Drop` impl since it can't propagate).
- Builder toggles on `FlameLayer`: `with_empty_samples`, `with_threads_collapsed`, `with_module_path`, `with_file_and_line`.

## Features

`default = ["smallvec"]`; the only feature, `smallvec`, just forwards to `tracing-subscriber/smallvec`.

## Testing

- `cargo nextest run -p tracing-flame` (or `cargo test -p tracing-flame --test concurrent`). `tempfile` is the only dev-dep.
- Both `tests/collapsed.rs` and `tests/concurrent.rs` call `set_global_default`, so each must run in its own process — nextest's per-test process isolation handles this; a plain `cargo test` cannot run them together. `concurrent.rs` asserts the output has exactly 5 lines, so changing the line format or empty-sample behavior will break it.

## Gotchas

- Flamegraph vs flamechart is purely an `inferno-flamegraph` flag (`--flamechart` preserves event order; default collapses/sorts identical frames). This crate only emits the folded text.
- With a buffered writer you **must** keep a `FlushGuard` alive (or call `.flush()`) before feeding the file to `inferno`, or the tail of the trace is lost.
