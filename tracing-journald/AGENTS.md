# AGENTS.md

`tracing-journald` is a `tracing_subscriber::Layer` that ships events to the systemd journal in journald's native protocol, preserving structured fields. Workspace-wide build/test/feature/commit conventions live in the root `AGENTS.md`.

## Platform constraint (read first)

This crate is meaningful only on systemd Linux. The Unix-only parts of `Layer` (`socket`, `socket_path`, and the `send_payload`/namespace logic) are `#[cfg(unix)]`; on non-Unix targets the constructors compile but return an `io::ErrorKind::NotFound`. The `memfd`/`socket` modules are `#[cfg(target_os = "linux")]`. Portable callers should treat a failing `layer()` as "journald absent" and fall back, not panic.

## Architecture

- `lib.rs` defines `Layer`, which implements `tracing_subscriber::Layer<S>`. It opens an unbound `UnixDatagram` and `send_to`s the journald socket (`/run/systemd/journal/socket` for `JournalNamespace::System`, `$XDG_RUNTIME_DIR/.../socket` for `User`). `on_new_span` serializes span fields into a `SpanFields(Vec<u8>)` extension; `on_event` replays each parent span's bytes (via `scope().from_root()`), appends `PRIORITY`/metadata/`SYSLOG_IDENTIFIER`/custom fields, then the event's own fields, and sends the buffer.
- Field encoding follows journald's native format: `put_field_wellformed` (newline form, no embedded newlines) for known-safe values, `put_field_length_encoded` (8-byte little-endian length prefix) for arbitrary values. `sanitize_name` enforces journald conventions — `.`->`_`, strip leading `_`, drop non-`[A-Za-z0-9_]`, upcase. User field names get a configurable prefix (default `"F"`); `message` maps to `MESSAGE` and is never prefixed. `TARGET`/`CODE_FILE`/`CODE_LINE` come from metadata; `SPAN_NAME` + `SPAN_`-prefixed metadata from spans.
- `Priority` (a `#[repr(u8)]` enum of ASCII `'0'`..`'7'`) and `PriorityMappings` define the `Level`->journald-priority map (default ERROR=3 … TRACE=7); override via `Layer::with_priority_mappings`. Other builders: `with_field_prefix`, `with_syslog_identifier`, `with_custom_fields`. Free fns `layer()` / `user_layer()` are shorthands for `Layer::new()` / `new_user()`.
- `socket.rs` `send_one_fd_to` passes a single fd as an `SCM_RIGHTS` control message via raw `sendmsg`. `memfd.rs` makes the `SYS_memfd_create` syscall directly (avoids the glibc≥2.27 `libc` wrapper) and fully seals the fd. Together they implement the oversized-payload path: when `send_to` returns `EMSGSIZE`, the payload is written to a sealed memfd and that fd is handed to journald.

## Testing

- `cargo nextest run -p tracing-journald`. Dev-deps: `serde`/`serde_json` (the test parses `journalctl -o json` back into structs).
- `tests/journal.rs` is `#![cfg(target_os = "linux")]` and **requires a running journald**: it constructs the layer and shells out to `journalctl` to read entries back. It skips gracefully when `journalctl --version` fails, but the system-journal cases call `Layer::new().unwrap()`, which panics if there is no journald socket — so the suite will fail in containers/CI without systemd. The user-journal cases skip cleanly if the user socket can't be opened.

## Gotchas

- Field-name sanitization is lossy and silent; two distinct tracing fields can collide into one journald key after mangling.
- The native length-encoded format means a value's bytes are written first and its length back-patched into the reserved 8-byte slot — `write_value` callbacks may only append to the buffer, never truncate it.
