//! Tests public formatting subscriber contracts.
#![cfg(feature = "fmt")]

#[cfg(test)]
mod tests {
  use std::error::Error;
  use std::fmt;
  use std::fmt::Write as FmtWrite;
  use std::io;
  use std::io::Write;
  use std::sync::Arc;
  use std::thread;

  use parking_lot::Mutex;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_lacks;
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
  fn default_formatter_includes_level_target_fields_and_message() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "INFO", "default formatter includes the event level")?;
    ensure_contains(&output, "fmt_contracts::default", "default formatter includes the event target")?;
    ensure_contains(&output, "hello default", "default formatter includes the message")?;
    ensure_contains(&output, "answer=42", "default formatter includes numeric fields")?;
    ensure_contains(&output, "enabled=true", "default formatter includes boolean fields")
  }

  #[test]
  fn full_formatter_renders_span_stack_source_and_named_thread() -> Result<(), TestFailure> {
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
      return ensure(false, "named formatter test thread should spawn");
    };
    if handle.join().is_err() {
      return ensure(false, "named formatter test thread should not panic");
    }

    let output = writer.output();
    ensure_contains(&output, "fmt-contract-thread", "full formatter includes named thread context")?;
    ensure_contains(&output, "ThreadId(", "full formatter includes thread IDs")?;
    ensure_contains(&output, "fmt_contracts.rs:", "full formatter includes source file and line")?;
    ensure_contains(&output, "full_root", "full formatter includes root span context")?;
    ensure_contains(&output, "root_field=\"omega\"", "full formatter includes root span fields")?;
    ensure_contains(&output, "full_child", "full formatter includes child span context")?;
    ensure_contains(&output, "child_field=7", "full formatter includes child span fields")?;
    ensure_contains(&output, "full child event", "full formatter includes event messages")?;
    ensure_contains(&output, "event_field=\"value\"", "full formatter includes event fields")
  }

  #[test]
  fn compact_formatter_respects_disabled_level_and_target_options() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "hello compact", "compact formatter includes the message")?;
    ensure_contains(&output, "request_id=\"abc\"", "compact formatter includes fields")?;
    ensure_lacks(&output, "WARN", "compact formatter omits disabled levels")?;
    ensure_lacks(&output, "fmt_contracts::compact", "compact formatter omits disabled targets")
  }

  #[test]
  fn compact_formatter_includes_source_location_and_current_span_fields() -> Result<(), TestFailure> {
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
      &output,
      "fmt_contracts::compact_full",
      "compact formatter includes targets when enabled",
    )?;
    ensure_contains(&output, "fmt_contracts.rs:", "compact formatter includes source file and line")?;
    ensure_contains(&output, "compact full event", "compact formatter writes event messages")?;
    ensure_contains(&output, "event_field=9", "compact formatter writes event fields")?;
    ensure_contains(&output, "compact_field=\"zeta\"", "compact formatter appends current span fields")
  }

  #[test]
  fn pretty_formatter_includes_span_context_and_fields() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "pretty_parent", "pretty formatter includes the current span name")?;
    ensure_contains(&output, "gamma", "pretty formatter includes span fields")?;
    ensure_contains(&output, "pretty child", "pretty formatter includes event messages")?;
    ensure_contains(&output, "503", "pretty formatter includes event fields")
  }

  #[test]
  fn pretty_formatter_styles_every_level_when_ansi_is_enabled() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "\u{1b}[", "pretty formatter emits ANSI escapes when enabled")?;
    ensure_contains(&output, "TRACE", "pretty formatter renders trace levels")?;
    ensure_contains(&output, "DEBUG", "pretty formatter renders debug levels")?;
    ensure_contains(&output, "INFO", "pretty formatter renders info levels")?;
    ensure_contains(&output, "WARN", "pretty formatter renders warn levels")?;
    ensure_contains(&output, "ERROR", "pretty formatter renders error levels")?;
    ensure_contains(&output, "pretty trace", "pretty formatter writes trace events")?;
    ensure_contains(&output, "pretty debug", "pretty formatter writes debug events")?;
    ensure_contains(&output, "pretty info", "pretty formatter writes info events")?;
    ensure_contains(&output, "pretty warn", "pretty formatter writes warn events")?;
    ensure_contains(&output, "pretty error", "pretty formatter writes error events")
  }

  #[test]
  fn pretty_formatter_places_line_number_after_target_when_file_is_disabled() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "fmt_contracts::line_only:", "pretty formatter keeps the event target")?;
    ensure_contains(&output, "line-only pretty event", "pretty formatter writes the event message")?;
    ensure_lacks(&output, "\n    at ", "pretty formatter omits file context when files are disabled")
  }

  #[test]
  fn pretty_formatter_renders_source_location_and_thread_context() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "source and thread context", "pretty formatter writes the event")?;
    ensure_contains(&output, "\n    at ", "pretty formatter writes source location context")?;
    ensure_contains(&output, "fmt_contracts.rs:", "pretty formatter includes the test file and line")?;
    ensure_contains(&output, " on ", "pretty formatter writes thread context")
  }

  #[test]
  fn pretty_formatter_renders_span_targets_and_explicit_parent_context() -> Result<(), TestFailure> {
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
      &output,
      "explicit parent pretty event",
      "pretty formatter writes explicit-parent events",
    )?;
    ensure_contains(
      &output,
      "fmt_contracts::span_target::targeted_parent",
      "pretty formatter includes span targets when targets are enabled",
    )?;
    ensure_contains(&output, "theta", "pretty formatter includes formatted parent span fields")?;
    ensure_contains(&output, "204", "pretty formatter includes explicit-parent event fields")
  }

  #[test]
  fn pretty_formatter_renders_error_sources() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "pretty error event", "pretty formatter writes error events")?;
    ensure_contains(&output, "outer failure", "pretty formatter writes the outer error")?;
    ensure_contains(&output, "error.sources", "pretty formatter labels error source chains")?;
    ensure_contains(&output, "root cause", "pretty formatter writes error sources")
  }

  #[test]
  fn custom_timer_and_disabled_timer_control_timestamp_output() -> Result<(), TestFailure> {
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
    ensure_contains(&timed_output, "fixed-time", "custom timer writes deterministic timestamps")?;
    ensure_contains(&timed_output, "timed event", "timed subscriber writes event messages")?;

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
    ensure_lacks(&untimed_output, "fixed-time", "disabled timer omits custom timestamps")?;
    ensure_contains(&untimed_output, "untimed event", "untimed subscriber still writes event messages")
  }

  #[test]
  fn custom_debug_fn_field_formatter_controls_event_and_span_fields() -> Result<(), TestFailure> {
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
      &output,
      "<message=custom fields event>",
      "custom debug formatter controls message output",
    )?;
    ensure_contains(&output, "<event_field=12>", "custom debug formatter controls event fields")?;
    ensure_contains(&output, "<span_field=\"phi\">", "custom debug formatter controls span fields")
  }

  #[test]
  fn span_lifecycle_events_include_configured_timing_fields() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "new", "span lifecycle output includes new-span events")?;
    ensure_contains(&output, "enter", "span lifecycle output includes enter events")?;
    ensure_contains(&output, "exit", "span lifecycle output includes exit events")?;
    ensure_contains(&output, "close", "span lifecycle output includes close events")?;
    ensure_contains(&output, "time.busy", "close events include busy timing")?;
    ensure_contains(&output, "time.idle", "close events include idle timing")?;
    ensure_contains(&output, "lifecycle_span", "span lifecycle output includes span names")?;
    ensure_contains(&output, "inside lifecycle", "normal events remain formatted")
  }

  #[test]
  fn fmt_span_bit_flags_report_public_debug_contract() -> Result<(), TestFailure> {
    let mut flags = FmtSpan::NEW | FmtSpan::ENTER;
    flags |= FmtSpan::EXIT;
    flags ^= FmtSpan::ENTER;
    flags &= FmtSpan::FULL;

    ensure_eq(
      &format!("{:?}", FmtSpan::NONE),
      &"FmtSpan::NONE".to_owned(),
      "empty span-event flags use the NONE debug spelling",
    )?;
    ensure_eq(
      &format!("{flags:?}"),
      &"FmtSpan::NEW | FmtSpan::EXIT".to_owned(),
      "combined span-event flags list enabled variants in order",
    )
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_formatter_emits_valid_event_span_and_field_shape() -> Result<(), TestFailure> {
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
      return ensure(false, "json formatter writes one line");
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "json output parses")?;

    ensure(
      value.get("level") == Some(&Value::String("INFO".to_owned())),
      "json formatter records the event level",
    )?;
    ensure(
      value.get("target") == Some(&Value::String("fmt_contracts::json".to_owned())),
      "json formatter records the event target",
    )?;

    let Some(fields) = value.get("fields").and_then(Value::as_object) else {
      return ensure(false, "json formatter records event fields as an object");
    };
    ensure(
      fields.get("message") == Some(&Value::String("json child".to_owned())),
      "json formatter records event messages in fields",
    )?;
    ensure(
      fields.get("answer") == Some(&Value::Number(42_u64.into())),
      "json formatter records numeric event fields",
    )?;

    let Some(span) = value.get("span").and_then(Value::as_object) else {
      return ensure(false, "json formatter records the current span");
    };
    ensure(
      span.get("name") == Some(&Value::String("json_parent".to_owned())),
      "json formatter records the current span name",
    )?;
    ensure(
      span.get("user") == Some(&Value::String("ada".to_owned())),
      "json formatter records current span fields",
    )?;

    let Some(spans) = value.get("spans").and_then(Value::as_array) else {
      return ensure(false, "json formatter records the active span stack");
    };
    ensure(spans.len() == 1, "json formatter records one active span")
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_event_format_factory_can_drive_subscriber_builder() -> Result<(), TestFailure> {
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
      return ensure(false, "json factory formatter writes one line");
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "json factory output parses")?;
    ensure(
      value.get("message") == Some(&Value::String("json factory event".to_owned())),
      "json factory formatter flattens messages",
    )?;
    ensure(
      value.get("factory_field") == Some(&Value::String("factory".to_owned())),
      "json factory formatter flattens fields",
    )?;
    ensure(
      value.get("target") == Some(&Value::String("fmt_contracts::json_factory".to_owned())),
      "json factory formatter keeps metadata",
    )
  }

  #[cfg(feature = "json")]
  #[test]
  fn json_formatter_flattening_and_span_options_change_output_shape() -> Result<(), TestFailure> {
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
      return ensure(false, "flattened json formatter writes one line");
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "flattened json parses")?;

    ensure(
      value.get("message") == Some(&Value::String("flattened json child".to_owned())),
      "flattened json promotes message to the root object",
    )?;
    ensure(
      value.get("answer") == Some(&Value::Number(7_u64.into())),
      "flattened json promotes numeric fields to the root object",
    )?;
    ensure(value.get("fields").is_none(), "flattened json omits nested fields object")?;
    ensure(value.get("span").is_none(), "disabled current-span output is omitted")?;
    ensure(value.get("spans").is_none(), "disabled span-list output is omitted")
  }

  #[test]
  fn fmt_layer_writer_accessors_and_registry_output() -> Result<(), TestFailure> {
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
    )?;
    *layer.writer_mut() = replacement_writer.clone();

    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, || {
      tracing::info!("layer writer mutation");
    });

    ensure_lacks(
      &original_writer.output(),
      "layer writer mutation",
      "mutating the layer writer leaves the original sink unused",
    )?;
    ensure_contains(
      &replacement_writer.output(),
      "layer writer mutation",
      "mutating the layer writer routes events to the replacement sink",
    )
  }

  #[test]
  fn fmt_layer_compact_lifecycle_options_render_registry_events() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "fixed-time", "layer timer writes deterministic timestamps")?;
    ensure_contains(&output, "ThreadId(", "layer thread-id option renders thread IDs")?;
    ensure_contains(&output, "fmt_contracts.rs:", "layer source-location options render source context")?;
    ensure_contains(&output, "layer_lifecycle", "layer renders span context")?;
    ensure_contains(&output, "layer_field=\"lambda\"", "layer renders span fields")?;
    ensure_contains(&output, "layer lifecycle event", "layer renders normal events")?;
    ensure_contains(&output, "event_field=\"inside\"", "layer renders event fields")?;
    ensure_contains(&output, "new", "layer synthesizes new-span events")?;
    ensure_contains(&output, "close", "layer synthesizes close-span events")?;
    ensure_contains(&output, "time.busy", "layer close events include busy timing")?;
    ensure_contains(&output, "time.idle", "layer close events include idle timing")
  }

  #[test]
  fn formatted_fields_public_accessors_round_trip() -> Result<(), TestFailure> {
    let mut fields = tracing_fmt::FormattedFields::<fmt_format::DefaultFields>::new("alpha=1".to_owned());
    ensure(fields.fields() == "alpha=1", "formatted fields expose their inner text")?;
    {
      let mut writer = fields.as_writer();
      ensure(
        FmtWrite::write_str(&mut writer, " beta=2").is_ok(),
        "formatted fields writer appends to the field string",
      )?;
    };

    ensure(
      fields.fields() == "alpha=1 beta=2",
      "formatted fields writer mutates the inner text",
    )?;
    ensure_eq(
      &fields.to_string(),
      &"alpha=1 beta=2".to_owned(),
      "formatted fields display as their inner text",
    )?;
    ensure(fields.starts_with("alpha"), "formatted fields deref to their inner string")?;
    let debug = format!("{fields:?}");
    ensure_contains(&debug, "FormattedFields", "formatted fields debug includes the type name")?;
    ensure_contains(&debug, "alpha=1 beta=2", "formatted fields debug includes the stored text")
  }

  #[cfg(feature = "ansi")]
  #[test]
  fn fmt_layer_pretty_mode_outputs_span_context_from_registry() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "layer_pretty_parent", "pretty layer renders span names")?;
    ensure_contains(&output, "rho", "pretty layer renders span fields")?;
    ensure_contains(&output, "layer pretty event", "pretty layer renders event messages")?;
    ensure_contains(&output, "409", "pretty layer renders event fields")
  }

  #[cfg(feature = "json")]
  #[test]
  fn fmt_layer_json_options_render_flattened_output_shape() -> Result<(), TestFailure> {
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
      return ensure(false, "flattened JSON layer writes one line");
    };
    let value: Value = ensure_ok(serde_json::from_str(first_line), "flattened JSON layer parses")?;
    ensure(
      value.get("message") == Some(&Value::String("flattened layer json child".to_owned())),
      "flattened JSON layer promotes event messages",
    )?;
    ensure(
      value.get("answer") == Some(&Value::Number(11_u64.into())),
      "flattened JSON layer promotes event fields",
    )?;
    ensure(value.get("fields").is_none(), "flattened JSON layer omits nested fields")?;
    ensure(value.get("span").is_none(), "JSON layer omits disabled current spans")?;
    ensure(value.get("spans").is_none(), "JSON layer omits disabled span lists")
  }

  #[test]
  fn ansi_disabled_removes_escape_sequences_from_formatted_events() -> Result<(), TestFailure> {
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
    ensure_contains(&output, "ansi-free event", "event is written")?;
    ensure_lacks(&output, "\u{1b}", "ANSI escape sequences are disabled")
  }

  #[test]
  fn writer_factory_routes_events_by_metadata() -> Result<(), TestFailure> {
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
    ensure_contains(&info_output, "info route", "info event is written to the default route")?;
    ensure_lacks(&info_output, "error route", "error event is absent from the default route")?;
    ensure_contains(&error_output, "error route", "error event is written to the error route")?;
    ensure_lacks(&error_output, "info route", "info event is absent from the error route")
  }
}
