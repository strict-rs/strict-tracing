//! This example demonstrates how `tracing` events can be recorded from within a
//! panic hook, capturing the span context in which the program panicked.
//!
//! The hook below is installed normally and records metadata from
//! `PanicHookInfo` if a real panic reaches it. The demonstration loop does not
//! create a panic; instead, it constructs a non-panicking demonstration record
//! inside the same tracing span and sends it through the shared recorder,
//! because intentional panics are not allowed in these examples.

use std::error::Error;
use std::panic as panic_runtime;

use tracing::Level;
use tracing::error;
use tracing::info;
use tracing::subscriber::set_global_default;
use tracing::trace;

/// Number that demonstrates panic recording while the span is active.
const DEMONSTRATION_RECORD_INPUT: i32 = 4;

/// Local representation shared by the panic hook and the safe demonstration.
struct PanicRecord {
  /// Text recorded for the panic or demonstration record.
  message:  String,
  /// Source location when one is available.
  location: Option<PanicLocation>,
  /// Origin of this record, either the hook or the demonstration path.
  source:   &'static str,
}

/// Source location recorded for a panic-like record.
struct PanicLocation {
  /// File path associated with the record.
  file:   String,
  /// Line number associated with the record.
  line:   u32,
  /// Column number associated with the record.
  column: u32,
}

/// Run the panic hook example.
fn main() -> Result<(), Box<dyn Error>> {
  let subscriber = tracing_subscriber::fmt().with_max_level(Level::TRACE).finish();

  // NOTE: Using `tracing` in a panic hook requires the use of the *global*
  // trace dispatcher (`tracing::subscriber::set_global_default`), rather than
  // the per-thread scoped dispatcher
  // (`tracing::subscriber::with_default`/`set_default`). With the scoped trace
  // dispatcher, the subscriber's thread-local context may already have been
  // torn down by unwinding by the time the panic handler is reached.
  set_global_default(subscriber)?;

  panic_runtime::set_hook(Box::new(record_panic));

  for number in 0..10 {
    check_number(number);
  }

  Ok(())
}

/// Record a panic with its source location when one is available.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the panic-hook callback separate from hook installation"
)]
fn record_panic(panic_info: &panic_runtime::PanicHookInfo<'_>) {
  let record = PanicRecord {
    message:  panic_info.to_string(),
    location: panic_info.location().map(|location| PanicLocation {
      file:   location.file().to_owned(),
      line:   location.line(),
      column: location.column(),
    }),
    source:   "hook",
  };

  record_panic_record(&record);
}

/// Record panic metadata from the hook or a safe demonstration record.
fn record_panic_record(record: &PanicRecord) {
  if let Some(location) = record.location.as_ref() {
    error!(
      message = record.message.as_str(),
      panic.source = record.source,
      panic.file = location.file.as_str(),
      panic.line = location.line,
      panic.column = location.column,
    );
  } else {
    error!(message = record.message.as_str(), panic.source = record.source,);
  }
}

/// Check one number and record a non-panicking demonstration event.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the instrumented check span visible around the demonstration record"
)]
#[tracing::instrument]
fn check_number(number: i32) {
  if number == DEMONSTRATION_RECORD_INPUT {
    let record = PanicRecord {
      message:  "non-panicking panic hook demonstration record".to_owned(),
      location: Some(PanicLocation {
        file:   file!().to_owned(),
        line:   line!(),
        column: column!(),
      }),
      source:   "demonstration",
    };

    record_panic_record(&record);
    trace!(number, "recorded non-panicking demonstration record");
  } else {
    info!(number, "number checks out");
  }
}
