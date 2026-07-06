//! Tests for forwarding `log` records into `tracing`.

#[cfg(test)]
mod tests {
  use std::num::NonZeroU64;
  use std::sync::Arc;

  use parking_lot::Mutex;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::subscriber::with_default;
  use tracing_core::Event;
  use tracing_core::Level;
  use tracing_core::LevelFilter;
  use tracing_core::Metadata;
  use tracing_core::Subscriber;
  use tracing_core::span;
  use tracing_core::span::Attributes;
  use tracing_core::span::Record;
  use tracing_core::subscriber::SubscriberResult;
  use tracing_log::LogTracer;
  use tracing_log::NormalizeEvent as _;

  /// One observed normalized metadata record.
  type NormalizedMetadataObservation = (bool, Option<OwnedMetadata>);

  /// Ordered log of normalized metadata observations.
  type NormalizedMetadataLog = Vec<NormalizedMetadataObservation>;

  /// Subscriber state captured by the test subscriber.
  struct State {
    /// Metadata observed from emitted events.
    normalized_metadata: Mutex<NormalizedMetadataLog>,
  }

  /// Owned projection of metadata fields used for equality assertions.
  #[derive(PartialEq, Debug, Clone)]
  struct OwnedMetadata {
    /// Metadata name.
    name:        String,
    /// Event target.
    target:      String,
    /// Event level.
    level:       Level,
    /// Optional module path.
    module_path: Option<String>,
    /// Optional source file.
    file:        Option<String>,
    /// Optional source line.
    line:        Option<u32>,
  }

  /// Subscriber that stores normalized metadata for every observed event.
  struct TestSubscriber(Arc<State>);

  impl Subscriber for TestSubscriber {
    fn enabled(&self, _meta: &Metadata<'_>) -> SubscriberResult<bool> {
      Ok(true)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
      Some(LevelFilter::from_level(Level::INFO))
    }

    fn new_span(&self, _span: &Attributes<'_>) -> SubscriberResult<span::Id> {
      Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
    }

    fn record(&self, _span: span::Id, _values: &Record<'_>) -> SubscriberResult {
      Ok(())
    }

    fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
      Ok(())
    }

    fn event(&self, event: &Event<'_>) -> SubscriberResult {
      self.0.normalized_metadata.lock().push((
        event.is_log(),
        event.normalized_metadata().map(|normalized| OwnedMetadata {
          name:        String::from(normalized.name()),
          target:      String::from(normalized.target()),
          level:       *normalized.level(),
          module_path: normalized.module_path().map(String::from),
          file:        normalized.file().map(String::from),
          line:        normalized.line(),
        }),
      ));

      Ok(())
    }

    fn enter(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }

    fn exit(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }
  }

  /// Normalize metadata for log-sourced events and ignore plain tracing events.
  #[test]
  fn normalized_metadata() -> Result<(), TestFailure> {
    ensure_ok(LogTracer::init(), "`LogTracer` should initialize")?;
    let me = Arc::new(State {
      normalized_metadata: Mutex::new(Vec::new()),
    });
    let state = Arc::clone(&me);

    with_default(TestSubscriber(me), || {
      log::info!("expected info log");
      log::debug!("unexpected debug log");
      let minimal_record = log::Record::builder()
        .args(format_args!("Error!"))
        .level(log::Level::Info)
        .build();
      log::logger().log(&minimal_record);
      last(
        &state,
        true,
        Some(&OwnedMetadata {
          name:        String::from("log event"),
          target:      String::new(),
          level:       Level::INFO,
          module_path: None,
          file:        None,
          line:        None,
        }),
      )?;

      let metadata_record = log::Record::builder()
        .args(format_args!("Error!"))
        .level(log::Level::Info)
        .target("log_tracer_target")
        .file(Some("server.rs"))
        .line(Some(144))
        .module_path(Some("log_tracer"))
        .build();
      log::logger().log(&metadata_record);
      last(
        &state,
        true,
        Some(&OwnedMetadata {
          name:        String::from("log event"),
          target:      String::from("log_tracer_target"),
          level:       Level::INFO,
          module_path: Some(String::from("log_tracer")),
          file:        Some(String::from("server.rs")),
          line:        Some(144),
        }),
      )?;

      state.normalized_metadata.lock().clear();
      tracing::info!("test with a tracing info");
      let found = {
        let lock = state.normalized_metadata.lock();
        lock.iter().any(|entry| !entry.0 && entry.1.as_ref().is_none())
      };
      ensure(found, "expected matching event in observed metadata")
    })
  }

  /// Assert that the last observed event has the expected log classification and metadata.
  fn last(state: &State, should_be_log: bool, expected: Option<&OwnedMetadata>) -> Result<(), TestFailure> {
    let (is_log, metadata) = {
      let lock = state.normalized_metadata.lock();
      ensure_some(lock.last().cloned(), "expected at least one event")?
    };
    ensure_eq(&is_log, &should_be_log, "event log classification matches")?;
    ensure(metadata.as_ref() == expected, "normalized metadata matches")
  }
}
