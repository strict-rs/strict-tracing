//! Behavior contracts for captured span traces and traced errors.

#[cfg(test)]
mod tests {
  use std::error::Error;
  use std::fmt;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::subscriber::with_default;
  use tracing_error::ErrorLayer;
  use tracing_error::SpanTrace;
  use tracing_error::SpanTraceStatus;
  use tracing_subscriber::prelude::*;

  #[derive(Debug)]
  struct LeafError;

  impl fmt::Display for LeafError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      formatter.write_str("leaf cause")
    }
  }

  impl Error for LeafError {}

  #[derive(Debug)]
  struct OuterError {
    source: LeafError,
  }

  impl OuterError {
    fn new() -> Self {
      Self {
        source: LeafError
      }
    }
  }

  impl fmt::Display for OuterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      formatter.write_str("outer failure")
    }
  }

  impl Error for OuterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
      Some(&self.source)
    }
  }

  fn with_error_layer<T>(run: impl FnOnce() -> Result<T, TestFailure>) -> Result<T, TestFailure> {
    let subscriber = tracing_subscriber::registry().with(ErrorLayer::default());
    with_default(subscriber, run)
  }

  fn capture_nested_trace() -> Result<SpanTrace, TestFailure> {
    with_error_layer(|| {
      let outer = tracing::info_span!("outer", answer = 42);
      let _outer_guard = outer.enter();
      let inner = tracing::info_span!("inner", mode = "fast");
      let _inner_guard = inner.enter();
      Ok(SpanTrace::capture())
    })
  }

  #[test]
  fn statuses_distinguish_empty_unsupported_and_captured_traces() -> Result<(), TestFailure> {
    let empty = SpanTrace::capture();
    ensure(empty.status() == SpanTraceStatus::EMPTY, "capture outside a span is empty")?;

    let explicit_empty = SpanTrace::new(tracing::Span::none());
    ensure(
      explicit_empty.status() == SpanTraceStatus::EMPTY,
      "explicit none span trace is empty",
    )?;

    with_default(tracing_subscriber::registry(), || -> Result<(), TestFailure> {
      let span = tracing::info_span!("unsupported");
      let _guard = span.enter();
      let unsupported = SpanTrace::capture();
      ensure(
        unsupported.status() == SpanTraceStatus::UNSUPPORTED,
        "registry without ErrorLayer cannot format span traces",
      )
    })?;

    with_error_layer(|| {
      let span = tracing::info_span!("captured");
      let _guard = span.enter();
      let captured = SpanTrace::capture();
      ensure(captured.status() == SpanTraceStatus::CAPTURED, "ErrorLayer captures active spans")
    })
  }

  #[test]
  fn with_spans_visits_leaf_to_root_and_stops_when_requested() -> Result<(), TestFailure> {
    let trace = capture_nested_trace()?;
    let mut visits = Vec::new();

    trace.with_spans(|metadata, fields| {
      visits.push((metadata.name().to_owned(), fields.to_owned()));
      true
    });

    ensure_eq(&visits.len(), &2_usize, "captured trace visits inner and outer spans")?;
    let first = ensure_some(visits.first(), "first visited span is present")?;
    ensure_eq(&first.0, &"inner".to_owned(), "with_spans starts at the captured leaf span")?;
    ensure_contains(&first.1, "mode=\"fast\"", "inner span fields are formatted")?;
    let second = ensure_some(visits.get(1), "second visited span is present")?;
    ensure_eq(&second.0, &"outer".to_owned(), "with_spans continues to the parent span")?;
    ensure_contains(&second.1, "answer=42", "outer span fields are formatted")?;

    let mut stopped = Vec::new();
    trace.with_spans(|metadata, fields| {
      stopped.push((metadata.name().to_owned(), fields.to_owned()));
      false
    });

    ensure_eq(&stopped.len(), &1_usize, "returning false stops span visitation")
  }

  #[test]
  fn display_and_debug_include_captured_span_context_only_when_supported() -> Result<(), TestFailure> {
    let trace = capture_nested_trace()?;
    let displayed = trace.to_string();
    ensure_contains(&displayed, "inner", "display includes inner span name")?;
    ensure_contains(&displayed, "outer", "display includes outer span name")?;
    ensure_contains(&displayed, "mode=\"fast\"", "display includes inner span fields")?;

    let debugged = format!("{trace:?}");
    ensure_contains(&debugged, "SpanTrace", "debug identifies the span trace type")?;
    ensure_contains(&debugged, "inner", "debug includes inner span name")?;
    ensure_contains(&debugged, "outer", "debug includes outer span name")?;

    let empty = SpanTrace::new(tracing::Span::none()).to_string();
    ensure(!empty.contains("inner"), "empty traces do not invent captured span names")?;

    let unsupported = with_default(tracing_subscriber::registry(), || -> Result<SpanTrace, TestFailure> {
      let span = tracing::info_span!("unsupported");
      let _guard = span.enter();
      Ok(SpanTrace::capture())
    })?;
    let unsupported_display = unsupported.to_string();
    ensure(
      !unsupported_display.contains("unsupported"),
      "unsupported traces do not invent captured span names",
    )
  }

  #[cfg(feature = "traced-error")]
  #[test]
  fn traced_errors_preserve_results_sources_and_extractable_span_traces() -> Result<(), TestFailure> {
    use tracing_error::ExtractSpanTrace as _;
    use tracing_error::InstrumentError as _;
    use tracing_error::InstrumentResult as _;

    let unrelated = LeafError;
    let unrelated_error: &(dyn Error + 'static) = &unrelated;
    ensure(unrelated_error.span_trace().is_none(), "unrelated errors do not expose span traces")?;

    let traced = with_error_layer(|| {
      let span = tracing::info_span!("error span", code = 7);
      let _guard = span.enter();
      Ok(OuterError::new().in_current_span())
    })?;
    ensure_eq(&traced.to_string(), &"outer failure".to_owned(), "traced error preserves display")?;
    ensure_contains(&format!("{traced:?}"), "OuterError", "traced error preserves debug")?;

    let traced_source = ensure_some(traced.source(), "traced error exposes span trace source")?;
    let span_trace = ensure_some(traced_source.span_trace(), "traced error source exposes span trace")?;
    ensure(
      span_trace.status() == SpanTraceStatus::CAPTURED,
      "extracted span trace remains captured",
    )?;
    let source_display = traced_source.to_string();
    ensure_contains(&source_display, "span backtrace", "traced source display names span backtrace")?;
    ensure_contains(&source_display, "error span", "traced source display includes captured span")?;
    let source_debug = format!("{traced_source:?}");
    ensure_contains(&source_debug, "span backtrace", "traced source debug names span backtrace")?;
    ensure_contains(&source_debug, "error span", "traced source debug includes captured span")?;
    let original_source = ensure_some(traced_source.source(), "traced error preserves original source chain")?;
    ensure_eq(
      &original_source.to_string(),
      &"leaf cause".to_owned(),
      "original source display is preserved",
    )?;

    let ok_result: Result<&'static str, OuterError> = Ok("ok");
    let ok_value = ensure_ok(ok_result.in_current_span(), "instrumented Ok result remains Ok")?;
    ensure_eq(&ok_value, &"ok", "instrumented Ok result preserves value")?;

    let err_result: Result<(), OuterError> = Err(OuterError::new());
    let result_error = ensure_some(err_result.in_current_span().err(), "instrumented Err result wraps the error")?;
    let result_source = ensure_some(result_error.source(), "instrumented result exposes span trace source")?;
    ensure(
      result_source.span_trace().is_some(),
      "instrumented result source exposes a span trace",
    )
  }
}
