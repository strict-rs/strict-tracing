//! Span trace capture, status, and formatting support.

use std::fmt;

use tracing::Metadata;
use tracing::Span;

use crate::layer::WithContext;

/// A captured trace of [`tracing`] spans.
///
/// This type can be thought of as a relative of
/// [`std::backtrace::Backtrace`][`Backtrace`].
/// However, rather than capturing the current call stack when it is
/// constructed, a `SpanTrace` instead captures the current [span] and its
/// [parents].
///
/// In many cases, span traces may be as useful as stack backtraces useful in
/// pinpointing where an error occurred and why, if not moreso:
///
/// * A span trace captures only the user-defined, human-readable `tracing` spans, rather than
///   _every_ frame in the call stack, often cutting out a lot of noise.
/// * Span traces include the [fields] recorded by each span in the trace, as well as their names
///   and source code location, so different invocations of a function can be distinguished,
/// * In asynchronous code, backtraces for errors that occur in [futures] often consist not of the
///   stack frames that _spawned_ a future, but the stack frames of the executor that is responsible
///   for running that future. This means that if an `async fn` calls another `async fn` which
///   generates an error, the calling async function will not appear in the stack trace (and often,
///   the callee won't either!). On the other hand, when the [`tracing-futures`] crate is used to
///   instrument async code, the span trace will represent the logical application context a future
///   was running in, rather than the stack trace of the executor that was polling a future when an
///   error occurred.
///
/// Finally, unlike stack [`Backtrace`]s, capturing a `SpanTrace` is fairly
/// lightweight, and the resulting struct is not large. The `SpanTrace` struct
/// is formatted lazily; instead, it simply stores a copy of the current span,
/// and allows visiting the spans in that span's trace tree by calling the
/// [`with_spans` method][`with_spans`].
///
/// # Formatting
///
/// The `SpanTrace` type implements `fmt::Display`, formatting the span trace
/// similarly to how Rust formats panics. For example:
///
/// ```text
///    0: custom_error::do_another_thing
///         with answer=42 will_succeed=false
///           at examples/examples/custom_error.rs:42
///    1: custom_error::do_something
///         with foo="hello world"
///           at examples/examples/custom_error.rs:37
/// ```
///
/// Additionally, if custom formatting is desired, the [`with_spans`] method can
/// be used to visit each span in the trace, formatting them in order.
///
/// [`Backtrace`]: std::backtrace::Backtrace
/// [span]: mod@tracing::span
/// [parents]: mod@tracing::span#span-relationships
/// [fields]: tracing::field
/// [futures]: std::future::Future
/// [`tracing-futures`]: https://docs.rs/tracing-futures/
/// [`with_spans`]: SpanTrace::with_spans()
#[derive(Clone)]
pub struct SpanTrace {
  /// The innermost captured span.
  span: Span,
}

// === impl SpanTrace ===

impl SpanTrace {
  /// Create a new span trace with the given span as the innermost span.
  #[must_use]
  #[allow(
    clippy::single_call_fn,
    reason = "public constructor remains the API for building a trace from an explicit span"
  )]
  pub const fn new(span: Span) -> Self {
    Self {
      span,
    }
  }

  /// Capture the current span trace.
  ///
  /// # Examples
  /// ```rust
  /// use tracing_error::SpanTrace;
  ///
  /// pub struct MyError {
  ///   span_trace: SpanTrace,
  ///   // ...
  /// }
  ///
  /// # fn some_error_condition() -> bool { true }
  ///
  /// pub fn my_function(arg: &str) -> Result<(), MyError> {
  ///   let span = tracing::info_span!("my_function", arg);
  ///   let _guard = span.enter();
  ///
  ///   if some_error_condition() {
  ///     return Err(MyError {
  ///       span_trace: SpanTrace::capture(),
  ///       // ...
  ///     });
  ///   }
  ///
  ///   // ...
  /// #   Ok(())
  /// }
  /// ```
  #[must_use]
  #[allow(
    clippy::single_call_fn,
    reason = "public capture entry point names the core SpanTrace operation"
  )]
  pub fn capture() -> Self {
    Self::new(Span::current())
  }

  /// Apply a function to all captured spans in the trace until it returns
  /// `false`.
  ///
  /// This will call the provided function with a reference to the
  /// [`Metadata`] and a formatted representation of the [fields] of each span
  /// captured in the trace, starting with the span that was current when the
  /// trace was captured. The function may return `true` or `false` to
  /// indicate whether to continue iterating over spans; if it returns
  /// `false`, no additional spans will be visited.
  ///
  /// [fields]: tracing::field
  /// [`Metadata`]: tracing::Metadata
  pub fn with_spans(&self, f: impl FnMut(&'static Metadata<'static>, &str) -> bool) {
    let _result = self.span.with_subscriber(|(id, subscriber)| {
      if let Some(getcx) = subscriber.downcast_ref::<WithContext>() {
        getcx.with_context(subscriber, id, f);
      }
    });
  }

  /// Returns the status of this `SpanTrace`.
  ///
  /// The status indicates one of the following:
  /// * the current subscriber does not support capturing `SpanTrace`s
  /// * there was no current span, so a trace was not captured
  /// * a span trace was successfully captured
  #[must_use]
  pub fn status(&self) -> SpanTraceStatus {
    let inner = if self.span.is_none() {
      SpanTraceStatusInner::Empty
    } else {
      let mut status = None;
      let _result = self.span.with_subscriber(|(_, subscriber)| {
        if subscriber.downcast_ref::<WithContext>().is_some() {
          status = Some(SpanTraceStatusInner::Captured);
        }
      });

      status.unwrap_or(SpanTraceStatusInner::Unsupported)
    };

    SpanTraceStatus(inner)
  }
}

/// The current status of a `SpanTrace`, indicating whether it was captured or
/// whether it is empty for some other reason.
#[derive(Debug, PartialEq, Eq)]
pub struct SpanTraceStatus(SpanTraceStatusInner);

impl SpanTraceStatus {
  /// Formatting a `SpanTrace` is not supported, likely because there is no
  /// `ErrorLayer` or the `ErrorLayer` is from a different version of
  /// `tracing_error`.
  pub const UNSUPPORTED: Self = Self(SpanTraceStatusInner::Unsupported);

  /// The `SpanTrace` is empty, likely because it was captured outside of any
  /// `span`s
  pub const EMPTY: Self = Self(SpanTraceStatusInner::Empty);

  /// A span trace has been captured and the `SpanTrace` should print
  /// reasonable information when rendered.
  pub const CAPTURED: Self = Self(SpanTraceStatusInner::Captured);
}

/// The representation for a [`SpanTraceStatus`].
#[derive(Debug, PartialEq, Eq)]
enum SpanTraceStatusInner {
  /// Capturing the current span trace is unsupported.
  Unsupported,
  /// No active span was captured.
  Empty,
  /// An active span trace was captured.
  Captured,
}

/// Return `false` from a span visitor when formatting fails.
macro_rules! try_bool {
  ($result:expr, $dest:ident) => {{
    let ret = $result.unwrap_or_else(|error| $dest = Err(error));

    if $dest.is_err() {
      return false;
    }

    ret
  }};
}

impl fmt::Display for SpanTrace {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut err = Ok(());
    let mut span: usize = 0;

    self.with_spans(|metadata, fields| {
      if span > 0 {
        try_bool!(f.write_str("\n"), err);
      }

      try_bool!(write!(f, "{:>4}: {}::{}", span, metadata.target(), metadata.name()), err);

      if !fields.is_empty() {
        try_bool!(write!(f, "\n           with {fields}"), err);
      }

      if let Some((file, line)) = metadata.file().zip(metadata.line()) {
        try_bool!(write!(f, "\n             at {file}:{line}"), err);
      }

      span = span.saturating_add(1);
      true
    });

    err
  }
}

impl fmt::Debug for SpanTrace {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    struct DebugSpan<'a> {
      metadata: &'a Metadata<'a>,
      fields:   &'a str,
    }

    impl fmt::Debug for DebugSpan<'_> {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{ target: {:?}, name: {:?}", self.metadata.target(), self.metadata.name())?;

        if !self.fields.is_empty() {
          write!(f, ", fields: {:?}", self.fields)?;
        }

        if let Some((file, line)) = self.metadata.file().zip(self.metadata.line()) {
          write!(f, ", file: {file:?}, line: {line:?}")?;
        }

        write!(f, " }}")?;

        Ok(())
      }
    }

    write!(f, "SpanTrace ")?;
    let mut dbg = f.debug_list();
    self.with_spans(|metadata, fields| {
      let _entry = dbg.entry(&DebugSpan {
        metadata,
        fields,
      });
      true
    });
    dbg.finish()
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use tracing::Level;
  use tracing::span;
  use tracing::subscriber::with_default;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry::Registry;

  use super::*;
  use crate::ErrorLayer;

  #[test]
  fn capture_supported() -> Result<(), TestFailure> {
    let subscriber = Registry::default().with(ErrorLayer::default());

    with_default(subscriber, || -> Result<(), TestFailure> {
      let span = span!(Level::ERROR, "test span");
      let _guard = span.enter();

      let span_trace = SpanTrace::capture();

      ensure(
        span_trace.status() == SpanTraceStatus::CAPTURED,
        "active span with error layer captures a span trace",
      )
    })
  }

  #[test]
  fn capture_empty() -> Result<(), TestFailure> {
    let subscriber = Registry::default().with(ErrorLayer::default());

    with_default(subscriber, || -> Result<(), TestFailure> {
      let span_trace = SpanTrace::capture();

      ensure(
        span_trace.status() == SpanTraceStatus::EMPTY,
        "error layer without an active span captures an empty trace",
      )
    })
  }

  #[test]
  fn capture_unsupported() -> Result<(), TestFailure> {
    let subscriber = Registry::default();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let span = span!(Level::ERROR, "test span");
      let _guard = span.enter();

      let span_trace = SpanTrace::capture();

      ensure(
        span_trace.status() == SpanTraceStatus::UNSUPPORTED,
        "subscriber without error layer cannot capture span traces",
      )
    })
  }
}
