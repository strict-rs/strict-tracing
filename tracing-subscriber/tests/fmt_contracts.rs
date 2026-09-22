//! Tests public formatting subscriber contracts.
#![cfg(feature = "fmt")]

#[cfg(test)]
mod tests {
  #[cfg(feature = "ansi")]
  use std::error::Error;
  use std::fmt;
  use std::fmt::Write as FmtWrite;
  use std::io;
  use std::io::Write;
  use std::sync::Arc;
  use std::thread;

  use parking_lot::Mutex;

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Retains the searched text and expected substring.
    #[error(transparent)]
    Substring(#[from] strict_test_support::SubstringFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonString(#[from] strict_test_support::ComparisonFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultSerdeJsonError(#[from] strict_test_support::ResultFailure<serde_json::Error>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_lacks;
  #[cfg(feature = "json")]
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing_core::Metadata;
  use tracing_core::field::Field;
  use tracing_subscriber::fmt as tracing_fmt;
  use tracing_subscriber::fmt::MakeWriter;
  use tracing_subscriber::fmt::Subscriber;
  use tracing_subscriber::fmt::format as fmt_format;
  use tracing_subscriber::fmt::format::FmtSpan;
  use tracing_subscriber::fmt::format::Writer;
  use tracing_subscriber::fmt::time::FormatTime;
  use tracing_subscriber::prelude::*;

  /// In-memory writer used to assert formatted output.
  #[derive(Clone, Debug, Default)]
  struct MemoryWriter {
    /// Shared output bytes.
    bytes: Arc<Mutex<Vec<u8>>>,
  }

  impl MemoryWriter {
    /// Returns the collected output as UTF-8 text.
    fn output(&self) -> String {
      String::from_utf8_lossy(&self.bytes.lock()).into_owned()
    }
  }

  impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      self.bytes.lock().extend_from_slice(buf);
      Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  impl<'writer> MakeWriter<'writer> for MemoryWriter {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
      Self::clone(self)
    }
  }

  /// Writer factory that routes high-severity events to a separate buffer.
  #[derive(Clone, Debug, Default)]
  struct RoutingWriter {
    /// Buffer for non-error events.
    info:  MemoryWriter,
    /// Buffer for error events.
    error: MemoryWriter,
  }

  impl RoutingWriter {
    /// Returns the output written by the non-error route.
    fn info_output(&self) -> String {
      self.info.output()
    }

    /// Returns the output written by the error route.
    fn error_output(&self) -> String {
      self.error.output()
    }
  }

  impl<'writer> MakeWriter<'writer> for RoutingWriter {
    type Writer = MemoryWriter;

    fn make_writer(&'writer self) -> Self::Writer {
      self.info.clone()
    }

    fn make_writer_for(&'writer self, meta: &Metadata<'_>) -> Self::Writer {
      if meta.level() <= &Level::ERROR {
        self.error.clone()
      } else {
        self.info.clone()
      }
    }
  }

  /// Deterministic timer used by formatter contract tests.
  #[derive(Debug)]
  struct StaticTimer;

  impl FormatTime for StaticTimer {
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
      FmtWrite::write_str(writer, "fixed-time")
    }
  }

  #[test]
  fn default_formatter_includes_level_target_fields_and_message() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      tracing::info!(target: "fmt_contracts::default", answer = 42, enabled = true, "hello default");
    });

    let output = writer.output();
    ensure_contains((output).clone(), String::from("INFO"), "default formatter includes the event level").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts::default"),
      "default formatter includes the event target",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("hello default"),
      "default formatter includes the message",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("answer=42"),
      "default formatter includes numeric fields",
    )
    .map(drop)?;
    ensure_contains(output, String::from("enabled=true"), "default formatter includes boolean fields")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn full_formatter_renders_span_stack_source_and_named_thread() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let thread_writer = writer.clone();
    let spawn = thread::Builder::new().name("fmt-contract-thread".to_owned()).spawn(move || {
      let subscriber = Subscriber::builder()
        .with_writer(thread_writer)
        .with_ansi(false)
        .without_time()
        .with_max_level(Level::TRACE)
        .with_thread_names(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .finish();

      with_default(subscriber, || {
        let root = tracing::info_span!("full_root", root_field = "omega");
        let _root_entered = root.enter();
        let child = tracing::debug_span!("full_child", child_field = 7_u64);
        let _child_entered = child.enter();
        tracing::debug!(target: "fmt_contracts::full", event_field = "value", "full child event");
      });
    });
    let Ok(handle) = spawn else {
      return ensure(false, "named formatter test thread should spawn")
        .map(drop)
        .map_err(TestError::from);
    };
    if handle.join().is_err() {
      return ensure(false, "named formatter test thread should not panic")
        .map(drop)
        .map_err(TestError::from);
    }

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("fmt-contract-thread"),
      "full formatter includes named thread context",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("ThreadId("), "full formatter includes thread IDs").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts.rs:"),
      "full formatter includes source file and line",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("full_root"),
      "full formatter includes root span context",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("root_field=\"omega\""),
      "full formatter includes root span fields",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("full_child"),
      "full formatter includes child span context",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("child_field=7"),
      "full formatter includes child span fields",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("full child event"),
      "full formatter includes event messages",
    )
    .map(drop)?;
    ensure_contains(
      output,
      String::from("event_field=\"value\""),
      "full formatter includes event fields",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn compact_formatter_respects_disabled_level_and_target_options() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .compact()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_level(false)
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      tracing::warn!(target: "fmt_contracts::compact", request_id = "abc", "hello compact");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("hello compact"),
      "compact formatter includes the message",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("request_id=\"abc\""),
      "compact formatter includes fields",
    )
    .map(drop)?;
    ensure_lacks((output).clone(), String::from("WARN"), "compact formatter omits disabled levels").map(drop)?;
    ensure_lacks(
      output,
      String::from("fmt_contracts::compact"),
      "compact formatter omits disabled targets",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn compact_formatter_includes_source_location_and_current_span_fields() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .compact()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_file(true)
      .with_line_number(true)
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("compact_parent", compact_field = "zeta");
      let _entered = span.enter();
      tracing::info!(target: "fmt_contracts::compact_full", event_field = 9_u64, "compact full event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts::compact_full"),
      "compact formatter includes targets when enabled",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts.rs:"),
      "compact formatter includes source file and line",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("compact full event"),
      "compact formatter writes event messages",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("event_field=9"),
      "compact formatter writes event fields",
    )
    .map(drop)?;
    ensure_contains(
      output,
      String::from("compact_field=\"zeta\""),
      "compact formatter appends current span fields",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_includes_span_context_and_fields() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("pretty_parent", request = "gamma");
      let _entered = span.enter();
      tracing::warn!(status = 503_u64, "pretty child");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("pretty_parent"),
      "pretty formatter includes the current span name",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("gamma"), "pretty formatter includes span fields").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("pretty child"),
      "pretty formatter includes event messages",
    )
    .map(drop)?;
    ensure_contains(output, String::from("503"), "pretty formatter includes event fields")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_styles_every_level_when_ansi_is_enabled() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(true)
      .without_time()
      .with_max_level(Level::TRACE)
      .finish();

    with_default(subscriber, || {
      tracing::trace!(target: "fmt_contracts::pretty_levels", "pretty trace");
      tracing::debug!(target: "fmt_contracts::pretty_levels", "pretty debug");
      tracing::info!(target: "fmt_contracts::pretty_levels", "pretty info");
      tracing::warn!(target: "fmt_contracts::pretty_levels", "pretty warn");
      tracing::error!(target: "fmt_contracts::pretty_levels", "pretty error");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("\u{1b}["),
      "pretty formatter emits ANSI escapes when enabled",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("TRACE"), "pretty formatter renders trace levels").map(drop)?;
    ensure_contains((output).clone(), String::from("DEBUG"), "pretty formatter renders debug levels").map(drop)?;
    ensure_contains((output).clone(), String::from("INFO"), "pretty formatter renders info levels").map(drop)?;
    ensure_contains((output).clone(), String::from("WARN"), "pretty formatter renders warn levels").map(drop)?;
    ensure_contains((output).clone(), String::from("ERROR"), "pretty formatter renders error levels").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("pretty trace"),
      "pretty formatter writes trace events",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("pretty debug"),
      "pretty formatter writes debug events",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("pretty info"), "pretty formatter writes info events").map(drop)?;
    ensure_contains((output).clone(), String::from("pretty warn"), "pretty formatter writes warn events").map(drop)?;
    ensure_contains(output, String::from("pretty error"), "pretty formatter writes error events")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_places_line_number_after_target_when_file_is_disabled() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_file(false)
      .with_line_number(true)
      .finish();

    with_default(subscriber, || {
      tracing::info!(target: "fmt_contracts::line_only", "line-only pretty event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts::line_only:"),
      "pretty formatter keeps the event target",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("line-only pretty event"),
      "pretty formatter writes the event message",
    )
    .map(drop)?;
    ensure_lacks(
      output,
      String::from("\n    at "),
      "pretty formatter omits file context when files are disabled",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_renders_source_location_and_thread_context() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_thread_names(true)
      .with_thread_ids(true)
      .finish();

    with_default(subscriber, || {
      tracing::warn!(target: "fmt_contracts::source_thread", "source and thread context");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("source and thread context"),
      "pretty formatter writes the event",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("\n    at "),
      "pretty formatter writes source location context",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts.rs:"),
      "pretty formatter includes the test file and line",
    )
    .map(drop)?;
    ensure_contains(output, String::from(" on "), "pretty formatter writes thread context")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_renders_span_targets_and_explicit_parent_context() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      let parent = tracing::info_span!(
        target: "fmt_contracts::span_target",
        "targeted_parent",
        parent_field = "theta"
      );
      tracing::warn!(parent: parent.id(), child_field = 204_u64, "explicit parent pretty event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("explicit parent pretty event"),
      "pretty formatter writes explicit-parent events",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts::span_target::targeted_parent"),
      "pretty formatter includes span targets when targets are enabled",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("theta"),
      "pretty formatter includes formatted parent span fields",
    )
    .map(drop)?;
    ensure_contains(
      output,
      String::from("204"),
      "pretty formatter includes explicit-parent event fields",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  #[cfg(feature = "ansi")]
  fn pretty_formatter_renders_error_sources() -> Result<(), TestError> {
    #[derive(Debug)]
    struct RootCause;

    impl fmt::Display for RootCause {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("root cause")
      }
    }

    impl Error for RootCause {}

    #[derive(Debug)]
    struct OuterError {
      source: RootCause,
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

    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      let error = OuterError {
        source: RootCause
      };
      let error_ref: &(dyn Error + 'static) = &error;
      tracing::error!(error = error_ref, "pretty error event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("pretty error event"),
      "pretty formatter writes error events",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("outer failure"),
      "pretty formatter writes the outer error",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("error.sources"),
      "pretty formatter labels error source chains",
    )
    .map(drop)?;
    ensure_contains(output, String::from("root cause"), "pretty formatter writes error sources")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn custom_timer_and_disabled_timer_control_timestamp_output() -> Result<(), TestError> {
    let timed_writer = MemoryWriter::default();
    let timed_subscriber = Subscriber::builder()
      .with_writer(timed_writer.clone())
      .with_ansi(false)
      .with_timer(StaticTimer)
      .with_level(false)
      .with_target(false)
      .finish();

    with_default(timed_subscriber, || {
      tracing::info!("timed event");
    });

    let timed_output = timed_writer.output();
    ensure_contains(
      (timed_output).clone(),
      String::from("fixed-time"),
      "custom timer writes deterministic timestamps",
    )
    .map(drop)?;
    ensure_contains(timed_output, String::from("timed event"), "timed subscriber writes event messages").map(drop)?;

    let untimed_writer = MemoryWriter::default();
    let untimed_subscriber = Subscriber::builder()
      .with_writer(untimed_writer.clone())
      .with_ansi(false)
      .without_time()
      .with_level(false)
      .with_target(false)
      .finish();

    with_default(untimed_subscriber, || {
      tracing::info!("untimed event");
    });

    let untimed_output = untimed_writer.output();
    ensure_lacks(
      (untimed_output).clone(),
      String::from("fixed-time"),
      "disabled timer omits custom timestamps",
    )
    .map(drop)?;
    ensure_contains(
      untimed_output,
      String::from("untimed event"),
      "untimed subscriber still writes event messages",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn custom_debug_fn_field_formatter_controls_event_and_span_fields() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let field_formatter = fmt_format::debug_fn(|field_writer: &mut Writer<'_>, field: &Field, value: &dyn fmt::Debug| {
      FmtWrite::write_fmt(field_writer, format_args!("<{}={value:?}>", field.name()))
    });
    let subscriber = Subscriber::builder()
      .fmt_fields(field_formatter)
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_level(false)
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("custom_fields_parent", span_field = "phi");
      let _entered = span.enter();
      tracing::info!(event_field = 12_u64, "custom fields event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("<message=custom fields event>"),
      "custom debug formatter controls message output",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("<event_field=12>"),
      "custom debug formatter controls event fields",
    )
    .map(drop)?;
    ensure_contains(
      output,
      String::from("<span_field=\"phi\">"),
      "custom debug formatter controls span fields",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn span_lifecycle_events_include_configured_timing_fields() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .with_timer(StaticTimer)
      .with_level(false)
      .with_target(false)
      .with_span_events(FmtSpan::FULL)
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("lifecycle_span", request = "delta");
      {
        let _entered = span.enter();
        tracing::info!("inside lifecycle");
      };
      drop(span);
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("new"),
      "span lifecycle output includes new-span events",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("enter"),
      "span lifecycle output includes enter events",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("exit"), "span lifecycle output includes exit events").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("close"),
      "span lifecycle output includes close events",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("time.busy"), "close events include busy timing").map(drop)?;
    ensure_contains((output).clone(), String::from("time.idle"), "close events include idle timing").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("lifecycle_span"),
      "span lifecycle output includes span names",
    )
    .map(drop)?;
    ensure_contains(output, String::from("inside lifecycle"), "normal events remain formatted")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn fmt_span_bit_flags_report_public_debug_contract() -> Result<(), TestError> {
    let mut flags = FmtSpan::NEW | FmtSpan::ENTER;
    flags |= FmtSpan::EXIT;
    flags ^= FmtSpan::ENTER;
    flags &= FmtSpan::FULL;

    ensure_eq(
      format!("{:?}", FmtSpan::NONE),
      "FmtSpan::NONE".to_owned(),
      "empty span-event flags use the NONE debug spelling",
    )
    .map(drop)?;
    ensure_eq(
      format!("{flags:?}"),
      "FmtSpan::NEW | FmtSpan::EXIT".to_owned(),
      "combined span-event flags list enabled variants in order",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_formatter_emits_valid_event_span_and_field_shape() -> Result<(), TestError> {
    use serde_json::Value;

    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .json()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("json_parent", user = "ada");
      let _entered = span.enter();
      tracing::info!(target: "fmt_contracts::json", answer = 42_u64, "json child");
    });

    let output = writer.output();
    let Some(first_line) = output.lines().next() else {
      return ensure(false, "json formatter writes one line")
        .map(drop)
        .map_err(TestError::from);
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "json output parses")?;

    ensure(
      value.get("level") == Some(&Value::String("INFO".to_owned())),
      "json formatter records the event level",
    )
    .map(drop)?;
    ensure(
      value.get("target") == Some(&Value::String("fmt_contracts::json".to_owned())),
      "json formatter records the event target",
    )
    .map(drop)?;

    let Some(fields) = value.get("fields").and_then(Value::as_object) else {
      return ensure(false, "json formatter records event fields as an object")
        .map(drop)
        .map_err(TestError::from);
    };
    ensure(
      fields.get("message") == Some(&Value::String("json child".to_owned())),
      "json formatter records event messages in fields",
    )
    .map(drop)?;
    ensure(
      fields.get("answer") == Some(&Value::Number(42_u64.into())),
      "json formatter records numeric event fields",
    )
    .map(drop)?;

    let Some(span) = value.get("span").and_then(Value::as_object) else {
      return ensure(false, "json formatter records the current span")
        .map(drop)
        .map_err(TestError::from);
    };
    ensure(
      span.get("name") == Some(&Value::String("json_parent".to_owned())),
      "json formatter records the current span name",
    )
    .map(drop)?;
    ensure(
      span.get("user") == Some(&Value::String("ada".to_owned())),
      "json formatter records current span fields",
    )
    .map(drop)?;

    let Some(spans) = value.get("spans").and_then(Value::as_array) else {
      return ensure(false, "json formatter records the active span stack")
        .map(drop)
        .map_err(TestError::from);
    };
    ensure(spans.len() == 1, "json formatter records one active span")
      .map(drop)
      .map_err(TestError::from)
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_event_format_factory_can_drive_subscriber_builder() -> Result<(), TestError> {
    use serde_json::Value;

    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .event_format(fmt_format::json().flatten_event(true))
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      tracing::warn!(target: "fmt_contracts::json_factory", factory_field = "factory", "json factory event");
    });

    let output = writer.output();
    let Some(first_line) = output.lines().next() else {
      return ensure(false, "json factory formatter writes one line")
        .map(drop)
        .map_err(TestError::from);
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "json factory output parses")?;
    ensure(
      value.get("message") == Some(&Value::String("json factory event".to_owned())),
      "json factory formatter flattens messages",
    )
    .map(drop)?;
    ensure(
      value.get("factory_field") == Some(&Value::String("factory".to_owned())),
      "json factory formatter flattens fields",
    )
    .map(drop)?;
    ensure(
      value.get("target") == Some(&Value::String("fmt_contracts::json_factory".to_owned())),
      "json factory formatter keeps metadata",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_formatter_flattening_and_span_options_change_output_shape() -> Result<(), TestError> {
    use serde_json::Value;

    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .json()
      .flatten_event(true)
      .with_current_span(false)
      .with_span_list(false)
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      let span = tracing::info_span!("hidden_json_parent", user = "grace");
      let _entered = span.enter();
      tracing::info!(answer = 7_u64, "flattened json child");
    });

    let output = writer.output();
    let Some(first_line) = output.lines().next() else {
      return ensure(false, "flattened json formatter writes one line")
        .map(drop)
        .map_err(TestError::from);
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "flattened json parses")?;

    ensure(
      value.get("message") == Some(&Value::String("flattened json child".to_owned())),
      "flattened json promotes message to the root object",
    )
    .map(drop)?;
    ensure(
      value.get("answer") == Some(&Value::Number(7_u64.into())),
      "flattened json promotes numeric fields to the root object",
    )
    .map(drop)?;
    ensure(value.get("fields").is_none(), "flattened json omits nested fields object").map(drop)?;
    ensure(value.get("span").is_none(), "disabled current-span output is omitted").map(drop)?;
    ensure(value.get("spans").is_none(), "disabled span-list output is omitted")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn fmt_layer_writer_accessors_and_registry_output() -> Result<(), TestError> {
    let original_writer = MemoryWriter::default();
    let replacement_writer = MemoryWriter::default();
    let mut layer = tracing_fmt::layer()
      .with_writer(original_writer.clone())
      .without_time()
      .with_level(false)
      .with_target(false)
      .with_ansi(false);

    ensure(
      layer.writer().output().is_empty(),
      "layer writer accessor exposes the configured writer",
    )
    .map(drop)?;
    *layer.writer_mut() = replacement_writer.clone();

    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, || {
      tracing::info!("layer writer mutation");
    });

    ensure_lacks(
      original_writer.output(),
      String::from("layer writer mutation"),
      "mutating the layer writer leaves the original sink unused",
    )
    .map(drop)?;
    ensure_contains(
      replacement_writer.output(),
      String::from("layer writer mutation"),
      "mutating the layer writer routes events to the replacement sink",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn fmt_layer_compact_lifecycle_options_render_registry_events() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let layer = tracing_fmt::layer()
      .compact()
      .with_writer(writer.clone())
      .with_timer(StaticTimer)
      .with_ansi(false)
      .with_thread_ids(true)
      .with_thread_names(false)
      .with_file(true)
      .with_line_number(true)
      .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
      let span = tracing::info_span!("layer_lifecycle", layer_field = "lambda");
      {
        let _entered = span.enter();
        tracing::info!(event_field = "inside", "layer lifecycle event");
      };
      drop(span);
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("fixed-time"),
      "layer timer writes deterministic timestamps",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("ThreadId("),
      "layer thread-id option renders thread IDs",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("fmt_contracts.rs:"),
      "layer source-location options render source context",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("layer_lifecycle"), "layer renders span context").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("layer_field=\"lambda\""),
      "layer renders span fields",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("layer lifecycle event"),
      "layer renders normal events",
    )
    .map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("event_field=\"inside\""),
      "layer renders event fields",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("new"), "layer synthesizes new-span events").map(drop)?;
    ensure_contains((output).clone(), String::from("close"), "layer synthesizes close-span events").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("time.busy"),
      "layer close events include busy timing",
    )
    .map(drop)?;
    ensure_contains(output, String::from("time.idle"), "layer close events include idle timing")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn formatted_fields_public_accessors_round_trip() -> Result<(), TestError> {
    let mut fields = tracing_fmt::FormattedFields::<fmt_format::DefaultFields>::new("alpha=1".to_owned());
    ensure(fields.fields() == "alpha=1", "formatted fields expose their inner text").map(drop)?;
    {
      let mut writer = fields.as_writer();
      ensure(
        FmtWrite::write_str(&mut writer, " beta=2").is_ok(),
        "formatted fields writer appends to the field string",
      )
      .map(drop)?;
    };

    ensure(
      fields.fields() == "alpha=1 beta=2",
      "formatted fields writer mutates the inner text",
    )
    .map(drop)?;
    ensure_eq(
      fields.to_string(),
      "alpha=1 beta=2".to_owned(),
      "formatted fields display as their inner text",
    )
    .map(drop)?;
    ensure(fields.starts_with("alpha"), "formatted fields deref to their inner string").map(drop)?;
    let debug = format!("{fields:?}");
    ensure_contains(
      (debug).clone(),
      String::from("FormattedFields"),
      "formatted fields debug includes the type name",
    )
    .map(drop)?;
    ensure_contains(
      debug,
      String::from("alpha=1 beta=2"),
      "formatted fields debug includes the stored text",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[cfg(feature = "ansi")]
  #[test]
  fn fmt_layer_pretty_mode_outputs_span_context_from_registry() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let layer = tracing_fmt::layer()
      .pretty()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_target(false);
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
      let span = tracing::info_span!("layer_pretty_parent", pretty_field = "rho");
      let _entered = span.enter();
      tracing::warn!(status = 409_u64, "layer pretty event");
    });

    let output = writer.output();
    ensure_contains(
      (output).clone(),
      String::from("layer_pretty_parent"),
      "pretty layer renders span names",
    )
    .map(drop)?;
    ensure_contains((output).clone(), String::from("rho"), "pretty layer renders span fields").map(drop)?;
    ensure_contains(
      (output).clone(),
      String::from("layer pretty event"),
      "pretty layer renders event messages",
    )
    .map(drop)?;
    ensure_contains(output, String::from("409"), "pretty layer renders event fields")
      .map(drop)
      .map_err(TestError::from)
  }

  #[cfg(feature = "json")]
  #[test]
  fn fmt_layer_json_options_render_flattened_output_shape() -> Result<(), TestError> {
    use serde_json::Value;

    let writer = MemoryWriter::default();
    let layer = tracing_fmt::layer()
      .json()
      .flatten_event(true)
      .with_current_span(false)
      .with_span_list(false)
      .with_writer(writer.clone())
      .without_time();
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
      let span = tracing::info_span!("hidden_layer_json_parent", user = "katherine");
      let _entered = span.enter();
      tracing::info!(answer = 11_u64, "flattened layer json child");
    });

    let output = writer.output();
    let Some(first_line) = output.lines().next() else {
      return ensure(false, "flattened JSON layer writes one line")
        .map(drop)
        .map_err(TestError::from);
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "flattened JSON layer parses")?;
    ensure(
      value.get("message") == Some(&Value::String("flattened layer json child".to_owned())),
      "flattened JSON layer promotes event messages",
    )
    .map(drop)?;
    ensure(
      value.get("answer") == Some(&Value::Number(11_u64.into())),
      "flattened JSON layer promotes event fields",
    )
    .map(drop)?;
    ensure(value.get("fields").is_none(), "flattened JSON layer omits nested fields").map(drop)?;
    ensure(value.get("span").is_none(), "JSON layer omits disabled current spans").map(drop)?;
    ensure(value.get("spans").is_none(), "JSON layer omits disabled span lists")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn ansi_disabled_removes_escape_sequences_from_formatted_events() -> Result<(), TestError> {
    let writer = MemoryWriter::default();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .finish();

    with_default(subscriber, || {
      tracing::error!(target: "fmt_contracts::ansi", "ansi-free event");
    });

    let output = writer.output();
    ensure_contains((output).clone(), String::from("ansi-free event"), "event is written").map(drop)?;
    ensure_lacks(output, String::from("\u{1b}"), "ANSI escape sequences are disabled")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn writer_factory_routes_events_by_metadata() -> Result<(), TestError> {
    let writer = RoutingWriter::default();
    let subscriber = Subscriber::builder()
      .with_writer(writer.clone())
      .with_ansi(false)
      .without_time()
      .with_level(false)
      .with_target(false)
      .finish();

    with_default(subscriber, || {
      tracing::info!("info route");
      tracing::error!("error route");
    });

    let info_output = writer.info_output();
    let error_output = writer.error_output();
    ensure_contains(
      (info_output).clone(),
      String::from("info route"),
      "info event is written to the default route",
    )
    .map(drop)?;
    ensure_lacks(
      info_output,
      String::from("error route"),
      "error event is absent from the default route",
    )
    .map(drop)?;
    ensure_contains(
      (error_output).clone(),
      String::from("error route"),
      "error event is written to the error route",
    )
    .map(drop)?;
    ensure_lacks(
      error_output,
      String::from("info route"),
      "info event is absent from the error route",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
