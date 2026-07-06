//! This example demonstrates overriding the way `tracing-subscriber`'s
//! `FmtSubscriber` formats fields on spans and events, using a closure.
//!
//! We'll create a custom format that prints key-value pairs separated by
//! colons rather than equals signs, and separates fields with commas.
//!
//! For an event like
//! ```rust
//! tracing::info!(hello = "world", answer = 42);
//! ```
//!
//! the default formatter will format the fields like this:
//! ```not_rust
//! hello="world" answer=42
//! ```
//! while our custom formatter will output
//! ```not_rust
//! hello: "world", answer: 42
//! ```
#![deny(rust_2018_idioms)]

#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

use std::error::Error;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Write as _;

/// Displays an erased field value with the representation used by `debug_fn`.
struct DebugValue<'value> {
  /// The erased field value to format.
  value: &'value dyn Debug,
}

impl Display for DebugValue<'_> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    Debug::fmt(self.value, formatter)
  }
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  use tracing_subscriber::fmt::format;
  use tracing_subscriber::prelude::*;

  // Format fields using the provided closure.
  let format = format::debug_fn(|writer, field, field_value| {
    // We'll format the field name and value separated with a colon.
    write!(writer, "{field}: {}", DebugValue { value: field_value })
  })
    // Separate each field with a comma.
    // This method is provided by an extension trait in the
    // `tracing-subscriber` prelude.
    .delimited(", ");

  // Create a `fmt` subscriber that uses our custom event format, and set it
  // as the default.
  tracing_subscriber::fmt().fmt_fields(format).try_init()?;

  // Shave some yaks!
  let number_of_yaks = 3;
  // this creates a new event, outside of any spans.
  tracing::info!(number_of_yaks, "preparing to shave yaks");

  let number_shaved = yak_shave::shave_all(number_of_yaks);
  tracing::info!(all_yaks_shaved = number_shaved == number_of_yaks, "yak shaving completed.");
  Ok(())
}
