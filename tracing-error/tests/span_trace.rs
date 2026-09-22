//! Behavior contracts for captured span traces and traced errors.

#[cfg(test)]
mod tests {
  use std::error::Error;
  use std::fmt;

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Retains the owning error when its source chain violates the contract.
    #[error("{context}: {error:?}")]
    MissingSource {
      /// Source-chain expectation that failed.
      context: &'static str,
      /// Complete owning error and its available sources.
      error:   tracing_error::TracedError<OuterError>,
    },
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonString(#[from] strict_test_support::ComparisonFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonUsize(#[from] strict_test_support::ComparisonFailure<usize, usize>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    OptionTracedErrorOuterError(#[from] strict_test_support::OptionFailure<tracing_error::TracedError<OuterError>>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracedErrorOuterError(#[from] strict_test_support::ResultFailure<tracing_error::TracedError<OuterError>>),
    /// Retains the searched text and expected substring.
    #[error(transparent)]
    Substring(#[from] strict_test_support::SubstringFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    OptionString(#[from] strict_test_support::OptionFailure<(String, String)>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    OptionSpanTrace(#[from] strict_test_support::OptionFailure<SpanTrace>),
  }

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

  fn with_error_layer<T>(run: impl FnOnce() -> Result<T, TestError>) -> Result<T, TestError> {
    let subscriber = tracing_subscriber::registry().with(ErrorLayer::default());
    with_default(subscriber, run)
  }

  fn capture_nested_trace() -> Result<SpanTrace, TestError> {
    with_error_layer(|| {
      let outer = tracing::info_span!("outer", answer = 42);
      let _outer_guard = outer.enter();
      let inner = tracing::info_span!("inner", mode = "fast");
      let _inner_guard = inner.enter();
      Ok(SpanTrace::capture())
    })
  }

  #[test]
  fn statuses_distinguish_empty_unsupported_and_captured_traces() -> Result<(), TestError> {
    let empty = SpanTrace::capture();
    ensure(empty.status() == SpanTraceStatus::EMPTY, "capture outside a span is empty").map(drop)?;

    let explicit_empty = SpanTrace::new(tracing::Span::none());
    ensure(
      explicit_empty.status() == SpanTraceStatus::EMPTY,
      "explicit none span trace is empty",
    )
    .map(drop)?;

    with_default(tracing_subscriber::registry(), || -> Result<(), TestError> {
      let span = tracing::info_span!("unsupported");
      let _guard = span.enter();
      let unsupported = SpanTrace::capture();
      ensure(
        unsupported.status() == SpanTraceStatus::UNSUPPORTED,
        "registry without ErrorLayer cannot format span traces",
      )
      .map(drop)
      .map_err(TestError::from)
    })?;

    with_error_layer(|| {
      let span = tracing::info_span!("captured");
      let _guard = span.enter();
      let captured = SpanTrace::capture();
      ensure(captured.status() == SpanTraceStatus::CAPTURED, "ErrorLayer captures active spans")
        .map(drop)
        .map_err(TestError::from)
    })
  }

  #[test]
  fn with_spans_visits_leaf_to_root_and_stops_when_requested() -> Result<(), TestError> {
    let trace = capture_nested_trace()?;
    let mut visits = Vec::new();

    trace.with_spans(|metadata, fields| {
      visits.push((metadata.name().to_owned(), fields.to_owned()));
      true
    });

    ensure_eq(visits.len(), 2_usize, "captured trace visits inner and outer spans").map(drop)?;
    let first = ensure_some(visits.first(), "first visited span is present").map_err(|failure| strict_test_support::OptionFailure {
      context: failure.context,
      option:  failure.option.cloned(),
    })?;
    ensure_eq((first.0).clone(), "inner".to_owned(), "with_spans starts at the captured leaf span").map(drop)?;
    ensure_contains((first.1).clone(), String::from("mode=\"fast\""), "inner span fields are formatted").map(drop)?;
    let second = ensure_some(visits.get(1), "second visited span is present").map_err(|failure| strict_test_support::OptionFailure {
      context: failure.context,
      option:  failure.option.cloned(),
    })?;
    ensure_eq((second.0).clone(), "outer".to_owned(), "with_spans continues to the parent span").map(drop)?;
    ensure_contains((second.1).clone(), String::from("answer=42"), "outer span fields are formatted").map(drop)?;

    let mut stopped = Vec::new();
    trace.with_spans(|metadata, fields| {
      stopped.push((metadata.name().to_owned(), fields.to_owned()));
      false
    });

    ensure_eq(stopped.len(), 1_usize, "returning false stops span visitation")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn display_and_debug_include_captured_span_context_only_when_supported() -> Result<(), TestError> {
    let trace = capture_nested_trace()?;
    let displayed = trace.to_string();
    ensure_contains((displayed).clone(), String::from("inner"), "display includes inner span name").map(drop)?;
    ensure_contains((displayed).clone(), String::from("outer"), "display includes outer span name").map(drop)?;
    ensure_contains(displayed, String::from("mode=\"fast\""), "display includes inner span fields").map(drop)?;

    let debugged = format!("{trace:?}");
    ensure_contains(
      (debugged).clone(),
      String::from("SpanTrace"),
      "debug identifies the span trace type",
    )
    .map(drop)?;
    ensure_contains((debugged).clone(), String::from("inner"), "debug includes inner span name").map(drop)?;
    ensure_contains(debugged, String::from("outer"), "debug includes outer span name").map(drop)?;

    let empty = SpanTrace::new(tracing::Span::none()).to_string();
    ensure(!empty.contains("inner"), "empty traces do not invent captured span names").map(drop)?;

    let unsupported = with_default(tracing_subscriber::registry(), || -> Result<SpanTrace, TestError> {
      let span = tracing::info_span!("unsupported");
      let _guard = span.enter();
      Ok(SpanTrace::capture())
    })?;
    let unsupported_display = unsupported.to_string();
    ensure(
      !unsupported_display.contains("unsupported"),
      "unsupported traces do not invent captured span names",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[cfg(feature = "traced-error")]
  #[test]
  fn traced_errors_preserve_results_sources_and_extractable_span_traces() -> Result<(), TestError> {
    use tracing_error::ExtractSpanTrace as _;
    use tracing_error::InstrumentError as _;
    use tracing_error::InstrumentResult as _;

    let unrelated = LeafError;
    let unrelated_error: &(dyn Error + 'static) = &unrelated;
    ensure(unrelated_error.span_trace().is_none(), "unrelated errors do not expose span traces").map(drop)?;

    let traced = with_error_layer(|| {
      let span = tracing::info_span!("error span", code = 7);
      let _guard = span.enter();
      Ok(OuterError::new().in_current_span())
    })?;
    ensure_eq(traced.to_string(), "outer failure".to_owned(), "traced error preserves display").map(drop)?;
    ensure_contains(format!("{traced:?}"), String::from("OuterError"), "traced error preserves debug").map(drop)?;

    let Some(traced_source) = traced.source() else {
      return Err(TestError::MissingSource {
        context: "traced error exposes span trace source",
        error:   traced,
      });
    };
    let span_trace = ensure_some(traced_source.span_trace(), "traced error source exposes span trace").map_err(|failure| {
      strict_test_support::OptionFailure {
        context: failure.context,
        option:  failure.option.cloned(),
      }
    })?;
    ensure(
      span_trace.status() == SpanTraceStatus::CAPTURED,
      "extracted span trace remains captured",
    )
    .map(drop)?;
    let source_display = traced_source.to_string();
    ensure_contains(
      (source_display).clone(),
      String::from("span backtrace"),
      "traced source display names span backtrace",
    )
    .map(drop)?;
    ensure_contains(
      source_display,
      String::from("error span"),
      "traced source display includes captured span",
    )
    .map(drop)?;
    let source_debug = format!("{traced_source:?}");
    ensure_contains(
      (source_debug).clone(),
      String::from("span backtrace"),
      "traced source debug names span backtrace",
    )
    .map(drop)?;
    ensure_contains(
      source_debug,
      String::from("error span"),
      "traced source debug includes captured span",
    )
    .map(drop)?;
    let Some(original_source) = traced_source.source() else {
      return Err(TestError::MissingSource {
        context: "traced error preserves original source chain",
        error:   traced,
      });
    };
    ensure_eq(
      original_source.to_string(),
      "leaf cause".to_owned(),
      "original source display is preserved",
    )
    .map(drop)?;

    let ok_result: Result<&'static str, OuterError> = Ok("ok");
    let ok_value = ensure_ok(ok_result.in_current_span(), "instrumented Ok result remains Ok")?;
    ensure_eq(String::from(ok_value), String::from("ok"), "instrumented Ok result preserves value").map(drop)?;

    let err_result: Result<(), OuterError> = Err(OuterError::new());
    let result_error = ensure_some(err_result.in_current_span().err(), "instrumented Err result wraps the error")?;
    let Some(result_source) = result_error.source() else {
      return Err(TestError::MissingSource {
        context: "instrumented result exposes span trace source",
        error:   result_error,
      });
    };
    ensure(
      result_source.span_trace().is_some(),
      "instrumented result source exposes a span trace",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
