//! This example demonstrates using the `tracing-error` crate's `SpanTrace` type
//! to attach a trace context to a custom error type.
#![deny(rust_2018_idioms)]
use std::error::Error;
use std::fmt;
use std::io::Write;
use std::io::stderr;
use std::io::stdout;
use std::io::{
  self,
};

use tracing_error::ErrorLayer;
use tracing_error::prelude::*;
use tracing_subscriber::fmt::layer as fmt_layer;
use tracing_subscriber::prelude::*;

/// Error type used to demonstrate attached span traces.
#[derive(Debug)]
struct FooError {
  /// Human-readable error summary.
  message: &'static str,
}

impl Error for FooError {}

impl fmt::Display for FooError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad(self.message)
  }
}

/// Enter a span and attach its span trace to any returned error.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the outer span separate from the nested failing operation"
)]
#[tracing::instrument]
fn do_something(foo: &str) -> Result<&'static str, impl Error + Send + Sync + 'static + use<>> {
  // Results can be instrumented with a `SpanTrace` via the `InstrumentResult` trait
  do_another_thing(42, false).in_current_span()
}

/// Produce an instrumented demonstration error.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the nested failing span visible in the span-trace output"
)]
#[tracing::instrument]
fn do_another_thing(answer: usize, will_succeed: bool) -> Result<&'static str, impl Error + Send + Sync + 'static> {
  // Errors can also be instrumented directly via the `InstrumentError` trait
  Err(
    FooError {
      message: "something broke, lol",
    }
    .in_current_span(),
  )
}

/// Run the instrumented error example.
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
    Err(error) => {
      writeln!(errors, "printing error chain naively")?;
      write_naive_spantraces(&error, &mut errors)?;

      writeln!(errors)?;
      writeln!(errors, "printing error with extract method")?;
      write_extracted_spantraces(&error, &mut errors)?;
    }
  }
  Ok(())
}

/// Write the source chain, rendering attached `SpanTrace`s specially.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the extracted span-trace rendering distinct from the naive rendering"
)]
fn write_extracted_spantraces(root_error: &(dyn Error + 'static), writer: &mut impl Write) -> io::Result<()> {
  let mut current_error = Some(root_error);
  let mut source_index = 0_usize;

  while let Some(error) = current_error {
    if let Some(spantrace) = error.span_trace() {
      writeln!(writer, "Span Backtrace:\n{spantrace}")?;
    } else {
      writeln!(writer, "Error {source_index}: {error}")?;
    }

    current_error = error.source();
    source_index = source_index.saturating_add(1);
  }

  Ok(())
}

/// Write each source uniformly without extracting attached `SpanTrace`s.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the naive span-trace rendering distinct from the extracted rendering"
)]
fn write_naive_spantraces(root_error: &(dyn Error + 'static), writer: &mut impl Write) -> io::Result<()> {
  let mut current_error = Some(root_error);
  let mut source_index = 0_usize;

  while let Some(error) = current_error {
    writeln!(writer, "Error {source_index}: {error}")?;
    current_error = error.source();
    source_index = source_index.saturating_add(1);
  }

  Ok(())
}
