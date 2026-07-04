//! This example demonstrates using the `tracing-error` crate's `SpanTrace` type
//! to attach a trace context to a custom error type.
#![deny(rust_2018_idioms)]
use std::error::Error;
use std::fmt;
use std::io::Write as _;
use std::io::stderr;
use std::io::stdout;

use tracing_error::ErrorLayer;
use tracing_error::SpanTrace;
use tracing_subscriber::fmt::layer as fmt_layer;
use tracing_subscriber::prelude::*;
/// Error type that stores the current span context.
#[derive(Debug)]
struct FooError {
  /// Human-readable error summary.
  message: &'static str,
  /// Captured span context formatted when the error is displayed.
  context: SpanTrace,
}

impl Error for FooError {}

impl fmt::Display for FooError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad(self.message)?;
    write!(f, "\n\nspan backtrace:\n{}", self.context)?;
    Ok(())
  }
}

/// Enter a span and delegate to the nested fallible operation.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the outer span separate from the nested failing operation"
)]
#[tracing::instrument]
fn do_something(foo: &str) -> Result<&'static str, impl Error + Send + Sync + 'static + use<>> {
  do_another_thing(42, false)
}

/// Return a demonstration error from inside a nested span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the nested failing span visible in the custom error output"
)]
#[tracing::instrument]
fn do_another_thing(answer: usize, will_succeed: bool) -> Result<&'static str, impl Error + Send + Sync + 'static + use<>> {
  Err(FooError {
    message: "something broke, lol",
    context: SpanTrace::capture(),
  })
}

/// Run the custom error example.
#[tracing::instrument]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  tracing_subscriber::registry()
        .with(fmt_layer())
        // The `ErrorLayer` subscriber layer enables the use of `SpanTrace`.
        .with(ErrorLayer::default())
        .try_init()?;
  let mut output = stdout().lock();
  let mut errors = stderr().lock();
  match do_something("hello world") {
    Ok(result) => writeln!(output, "did something successfully: {result}")?,
    Err(error) => writeln!(errors, "error: {error}")?,
  }
  Ok(())
}
