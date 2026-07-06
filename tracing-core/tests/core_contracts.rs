//! Public data-model and dispatcher contract coverage.

#[cfg(test)]
mod tests {
  use std::error::Error;
  use std::fmt;
  use std::num::NonZeroU64;
  use std::sync::Arc;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_lacks;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing_core::Dispatch;
  use tracing_core::Event;
  use tracing_core::Kind;
  use tracing_core::Level;
  use tracing_core::LevelFilter;
  use tracing_core::Metadata;
  use tracing_core::Subscriber;
  use tracing_core::callsite::Callsite as _;
  use tracing_core::callsite::DefaultCallsite;
  use tracing_core::field;
  use tracing_core::field::Field;
  use tracing_core::field::FieldSet;
  use tracing_core::field::Value;
  use tracing_core::field::Visit;
  use tracing_core::metadata::SourceLocation;
  use tracing_core::span;
  use tracing_core::subscriber::Interest;
  use tracing_core::subscriber::SubscriberError;
  use tracing_core::subscriber::SubscriberResult;
  use tracing_core::test_util::NoOpSubscriber;
  use tracing_core::test_util::Primary;

  /// Event callsite used by public metadata and field tests.
  static EVENT_CALLSITE: DefaultCallsite = {
    static META: Metadata<'static> = Metadata::new(
      "core_contract_event",
      "core_contracts::event",
      Level::INFO,
      &SourceLocation::empty()
        .with_module_path(Some("core_contracts"))
        .with_file(Some("core_contracts.rs"))
        .with_line(Some(11)),
      &FieldSet::new(&["message", "answer", "enabled"], tracing_core::identify_callsite!(&EVENT_CALLSITE)),
      Kind::EVENT,
    );
    DefaultCallsite::new(&META)
  };

  /// Span callsite used by public metadata and dispatcher tests.
  static SPAN_CALLSITE: DefaultCallsite = {
    static META: Metadata<'static> = Metadata::new(
      "core_contract_span",
      "core_contracts::span",
      Level::DEBUG,
      &SourceLocation::empty().with_file(Some("core_contracts.rs")),
      &FieldSet::new(&["span_field"], tracing_core::identify_callsite!(&SPAN_CALLSITE)),
      Kind::SPAN,
    );
    DefaultCallsite::new(&META)
  };

  /// Secondary callsite used to prove field identity includes callsite identity.
  static OTHER_CALLSITE: DefaultCallsite = {
    static META: Metadata<'static> = Metadata::new(
      "core_contract_other",
      "core_contracts::other",
      Level::WARN,
      &SourceLocation::empty().with_line(Some(99)),
      &FieldSet::new(&["message"], tracing_core::identify_callsite!(&OTHER_CALLSITE)),
      Kind::EVENT,
    );
    DefaultCallsite::new(&META)
  };

  /// Subscriber hook counts used by dispatch-forwarding tests.
  #[derive(Debug, Default)]
  struct HookCounts {
    /// Number of `on_register_dispatch` hooks.
    register_dispatch: AtomicUsize,
    /// Number of callsite-registration hooks.
    register_callsite: AtomicUsize,
    /// Number of static enabled checks.
    enabled:           AtomicUsize,
    /// Number of span-construction hooks.
    new_span:          AtomicUsize,
    /// Number of record hooks.
    record:            AtomicUsize,
    /// Number of follows-from hooks.
    follows_from:      AtomicUsize,
    /// Number of event-enabled hooks.
    event_enabled:     AtomicUsize,
    /// Number of event hooks.
    event:             AtomicUsize,
    /// Number of enter hooks.
    enter:             AtomicUsize,
    /// Number of exit hooks.
    exit:              AtomicUsize,
    /// Number of clone-span hooks.
    clone_span:        AtomicUsize,
    /// Number of try-close hooks.
    try_close:         AtomicUsize,
    /// Number of current-span hooks.
    current_span:      AtomicUsize,
  }

  /// Subscriber fixture that records every public dispatch hook.
  #[derive(Debug)]
  struct RecordingSubscriber {
    /// Shared hook counts.
    counts:  Arc<HookCounts>,
    /// Static enabled decision.
    enabled: bool,
  }

  impl Subscriber for RecordingSubscriber {
    fn on_register_dispatch(&self, _subscriber: &Dispatch) -> SubscriberResult {
      let _previous = self.counts.register_dispatch.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
      let _previous = self.counts.register_callsite.fetch_add(1, Ordering::SeqCst);
      Ok(Interest::sometimes())
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
      let _previous = self.counts.enabled.fetch_add(1, Ordering::SeqCst);
      Ok(self.enabled)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
      Some(LevelFilter::INFO)
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
      let _previous = self.counts.new_span.fetch_add(1, Ordering::SeqCst);
      Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
    }

    fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
      let _previous = self.counts.record.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
      let _previous = self.counts.follows_from.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn event_enabled(&self, _event: &Event<'_>) -> SubscriberResult<bool> {
      let _previous = self.counts.event_enabled.fetch_add(1, Ordering::SeqCst);
      Ok(true)
    }

    fn event(&self, _event: &Event<'_>) -> SubscriberResult {
      let _previous = self.counts.event.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn enter(&self, _span: span::Id) -> SubscriberResult {
      let _previous = self.counts.enter.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn exit(&self, _span: span::Id) -> SubscriberResult {
      let _previous = self.counts.exit.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn clone_span(&self, id: span::Id) -> SubscriberResult<span::Id> {
      let _previous = self.counts.clone_span.fetch_add(1, Ordering::SeqCst);
      Ok(id)
    }

    fn try_close(&self, _id: span::Id) -> SubscriberResult<bool> {
      let _previous = self.counts.try_close.fetch_add(1, Ordering::SeqCst);
      Ok(true)
    }

    fn current_span(&self) -> SubscriberResult<span::Current> {
      let _previous = self.counts.current_span.fetch_add(1, Ordering::SeqCst);
      Ok(span::Current::new(
        span::Id::from_non_zero_u64(NonZeroU64::MIN),
        SPAN_CALLSITE.metadata(),
      ))
    }
  }

  /// Builds a recording dispatch and returns its shared hook counts.
  fn recording_dispatch(enabled: bool) -> (Dispatch, Arc<HookCounts>) {
    let counts = Arc::new(HookCounts::default());
    let dispatch = Dispatch::new(RecordingSubscriber {
      counts: Arc::clone(&counts),
      enabled,
    });
    (dispatch, counts)
  }

  /// Visitor that records typed field observations.
  #[derive(Debug, Default)]
  struct RecordingVisitor {
    /// Recorded field/value pairs.
    entries: Vec<String>,
  }

  impl Visit for RecordingVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
      self.entries.push(format!("{}={value}", field.name()));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
      self.entries.push(format!("{}={value}", field.name()));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
      self.entries.push(format!("{}={value}", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
      self.entries.push(format!("{}={value}", field.name()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
      self.entries.push(format!("{}={value:?}", field.name()));
    }
  }

  /// Error fixture used by field-value tests.
  #[derive(Debug)]
  struct FieldError;

  impl fmt::Display for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      formatter.write_str("field error")
    }
  }

  impl Error for FieldError {}

  #[test]
  fn metadata_source_location_kind_and_debug_contracts() -> Result<(), TestFailure> {
    let metadata = EVENT_CALLSITE.metadata();
    ensure_eq(&metadata.name(), &"core_contract_event", "metadata preserves names")?;
    ensure_eq(&metadata.target(), &"core_contracts::event", "metadata preserves targets")?;
    ensure_eq(metadata.level(), &Level::INFO, "metadata preserves levels")?;
    ensure(metadata.module_path() == Some("core_contracts"), "metadata preserves module paths")?;
    ensure(metadata.file() == Some("core_contracts.rs"), "metadata preserves files")?;
    ensure(metadata.line() == Some(11), "metadata preserves lines")?;
    ensure(metadata.is_event(), "event metadata reports event kind")?;
    ensure(!metadata.is_span(), "event metadata does not report span kind")?;

    let fields = metadata.fields();
    ensure_eq(&fields.len(), &3, "metadata exposes its field count")?;
    ensure(!fields.is_empty(), "metadata with fields is not empty")?;
    let message = ensure_some(fields.field("message"), "message field is present")?;
    ensure_eq(&message.name(), &"message", "field exposes its name")?;
    ensure_eq(&message.index(), &0, "field exposes its index")?;
    ensure(fields.contains(&message), "field set contains its own field")?;
    let fake = metadata.private_fake_field();
    ensure_eq(&fake.name(), &"<unknown>", "private fake fields expose the unknown name")?;
    ensure(!fields.contains(&fake), "field set rejects fake fields")?;

    let event_hint = Kind::EVENT.hint();
    ensure(event_hint.is_event(), "event hints preserve the event bit")?;
    ensure(event_hint.is_hint(), "event hints set the hint bit")?;
    ensure(!event_hint.is_span(), "event hints do not set the span bit")?;
    ensure_eq(
      &format!("{event_hint:?}"),
      &"Kind(EVENT | HINT)".to_owned(),
      "kind debug reports combined bits",
    )?;

    let full_debug = format!("{metadata:?}");
    ensure_contains(&full_debug, "module_path", "metadata debug includes populated module paths")?;
    ensure_contains(&full_debug, "location", "metadata debug combines populated file and line")?;
    ensure_contains(&full_debug, "Kind(EVENT)", "metadata debug includes kind names")?;

    let file_only = Metadata::new(
      "file_only",
      "core_contracts::file_only",
      Level::WARN,
      &SourceLocation::empty().with_file(Some("only_file.rs")),
      fields,
      Kind::SPAN,
    );
    let line_only = Metadata::new(
      "line_only",
      "core_contracts::line_only",
      Level::ERROR,
      &SourceLocation::empty().with_line(Some(77)),
      fields,
      Kind::EVENT,
    );
    ensure_contains(
      &format!("{file_only:?}"),
      "only_file.rs",
      "metadata debug includes file-only locations",
    )?;
    ensure_contains(&format!("{line_only:?}"), "line: 77", "metadata debug includes line-only locations")?;
    ensure_lacks(
      &format!("{:?}", SourceLocation::empty()),
      "core_contracts",
      "empty source locations do not invent components",
    )
  }

  #[test]
  fn level_and_level_filter_public_conversions_are_stable() -> Result<(), TestFailure> {
    let parsed_levels = [
      ("error", Level::ERROR, "ERROR"),
      ("WARN", Level::WARN, "WARN"),
      ("3", Level::INFO, "INFO"),
      ("4", Level::DEBUG, "DEBUG"),
      ("trace", Level::TRACE, "TRACE"),
    ];
    for &(input, expected, rendered) in &parsed_levels {
      let parsed = ensure_ok(input.parse::<Level>(), "level input parses")?;
      ensure_eq(&parsed, &expected, "level input maps to the expected level")?;
      ensure_eq(&parsed.as_str(), &rendered, "level as_str matches display spelling")?;
      ensure_eq(&parsed.to_string(), &rendered.to_owned(), "level display matches as_str spelling")?;
    }
    ensure("0".parse::<Level>().is_err(), "zero is not a valid Level")?;
    ensure("verbose".parse::<Level>().is_err(), "unknown strings are not valid Levels")?;

    let parsed_filters = [
      ("0", LevelFilter::OFF, None, "off", "LevelFilter::OFF"),
      ("", LevelFilter::ERROR, Some(Level::ERROR), "error", "LevelFilter::ERROR"),
      ("2", LevelFilter::WARN, Some(Level::WARN), "warn", "LevelFilter::WARN"),
      ("info", LevelFilter::INFO, Some(Level::INFO), "info", "LevelFilter::INFO"),
      ("debug", LevelFilter::DEBUG, Some(Level::DEBUG), "debug", "LevelFilter::DEBUG"),
      ("TRACE", LevelFilter::TRACE, Some(Level::TRACE), "trace", "LevelFilter::TRACE"),
    ];
    for &(input, expected, expected_level, display, debug) in &parsed_filters {
      let parsed = ensure_ok(input.parse::<LevelFilter>(), "level-filter input parses")?;
      ensure_eq(&parsed, &expected, "level-filter input maps to the expected filter")?;
      ensure(parsed.into_level() == expected_level, "level-filter converts into expected level")?;
      ensure_eq(&parsed.to_string(), &display.to_owned(), "level-filter display is stable")?;
      ensure_eq(&format!("{parsed:?}"), &debug.to_owned(), "level-filter debug is stable")?;
    }
    ensure("6".parse::<LevelFilter>().is_err(), "out-of-range numeric filters fail")?;
    ensure("verbose".parse::<LevelFilter>().is_err(), "unknown filter strings fail")?;
    ensure(LevelFilter::INFO.enables(Level::INFO), "filters enable their own level")?;
    ensure(LevelFilter::INFO.enables(Level::ERROR), "filters enable less verbose levels")?;
    ensure(!LevelFilter::INFO.enables(Level::DEBUG), "filters reject more verbose levels")?;
    ensure(!LevelFilter::OFF.enables(Level::ERROR), "off filters reject every level")?;

    let from_level: LevelFilter = Level::TRACE.into();
    let from_option: LevelFilter = Some(Level::WARN).into();
    let off_from_option: LevelFilter = None.into();
    ensure_eq(&from_level, &LevelFilter::TRACE, "levels convert into matching filters")?;
    ensure_eq(&from_option, &LevelFilter::WARN, "some levels convert into matching filters")?;
    ensure_eq(&off_from_option, &LevelFilter::OFF, "none converts into the off filter")
  }

  #[test]
  fn field_sets_and_value_sets_record_only_declared_fields() -> Result<(), TestFailure> {
    let fields = EVENT_CALLSITE.metadata().fields();
    let message = ensure_some(fields.field("message"), "message field exists")?;
    let answer = ensure_some(fields.field("answer"), "answer field exists")?;
    let enabled = ensure_some(fields.field("enabled"), "enabled field exists")?;
    let other_message = ensure_some(OTHER_CALLSITE.metadata().fields().field("message"), "other field exists")?;

    let message_value: &dyn Value = &"hello";
    let answer_value: &dyn Value = &42_u64;
    let enabled_value: &dyn Value = &true;
    let other_value: &dyn Value = &"ignored";
    let values = [
      (&message, Some(message_value)),
      (&other_message, Some(other_value)),
      (&answer, Some(answer_value)),
      (&enabled, Some(enabled_value)),
    ];
    let value_set = fields.value_set(&values);
    let mut visitor = RecordingVisitor::default();
    value_set.record(&mut visitor);

    ensure_eq(&value_set.len(), &3, "value sets count only fields from their own callsite")?;
    ensure(!value_set.is_empty(), "value sets with present declared values are not empty")?;
    ensure(
      visitor.entries == vec!["message=hello".to_owned(), "answer=42".to_owned(), "enabled=true".to_owned()],
      "value sets record declared field values in supplied order",
    )?;
    ensure_contains(&format!("{value_set}"), "message", "value-set display includes recorded fields")?;
    ensure_contains(&format!("{value_set:?}"), "callsite", "value-set debug includes callsite identity")?;

    let all_values = [Some(message_value), None, Some(enabled_value), Some(other_value)];
    let value_set_all = fields.value_set_all(&all_values);
    let mut all_visitor = RecordingVisitor::default();
    value_set_all.record(&mut all_visitor);
    ensure_eq(&value_set_all.len(), &2, "positional value sets count present fields")?;
    ensure(
      all_visitor.entries == vec!["message=hello".to_owned(), "enabled=true".to_owned()],
      "positional value sets zip values to declared fields and ignore extras",
    )?;

    let none_value: Option<u64> = None;
    let none_value_ref: &dyn Value = &none_value;
    let none_values = [(&answer, Some(none_value_ref))];
    let none_set = fields.value_set(&none_values);
    let mut none_visitor = RecordingVisitor::default();
    none_set.record(&mut none_visitor);
    ensure(none_visitor.entries.is_empty(), "none field values do not record")?;

    let display_value: &dyn Value = &field::display("shown");
    let debug_value: &dyn Value = &field::debug(["dbg"]);
    let error = FieldError;
    let error_value: &(dyn Error + 'static) = &error;
    let error_field_value: &dyn Value = &error_value;
    let formatted_values = [
      (&message, Some(display_value)),
      (&answer, Some(debug_value)),
      (&enabled, Some(error_field_value)),
    ];
    let mut formatted_visitor = RecordingVisitor::default();
    fields.value_set(&formatted_values).record(&mut formatted_visitor);
    ensure_contains(
      &formatted_visitor.entries.join(","),
      "message=shown",
      "display field values record their display text",
    )?;
    ensure_contains(
      &formatted_visitor.entries.join(","),
      "answer=[\"dbg\"]",
      "debug field values record their debug text",
    )?;
    ensure_contains(
      &formatted_visitor.entries.join(","),
      "enabled=field error",
      "error field values record their display text",
    )
  }

  #[test]
  fn dispatch_debug_downcast_and_weak_lifetime_contracts() -> Result<(), TestFailure> {
    let (dispatch, counts) = recording_dispatch(true);
    ensure(
      dispatch.is::<RecordingSubscriber>(),
      "dispatch reports its concrete subscriber type",
    )?;
    ensure(
      dispatch.downcast_ref::<RecordingSubscriber>().is_some(),
      "dispatch downcasts to the concrete subscriber",
    )?;
    ensure(
      !dispatch.is::<NoOpSubscriber<Primary>>(),
      "dispatch rejects unrelated subscriber types",
    )?;
    ensure_contains(
      &format!("{dispatch:?}"),
      "Dispatch::Scoped",
      "dispatch debug identifies scoped dispatches",
    )?;
    ensure_contains(
      &format!("{:?}", Dispatch::none()),
      "Dispatch::Global",
      "none dispatch debug identifies global dispatches",
    )?;

    let weak = dispatch.downgrade();
    ensure_contains(
      &format!("{weak:?}"),
      "WeakDispatch::Scoped",
      "weak dispatch debug identifies scoped dispatches",
    )?;
    let upgraded = weak.upgrade();
    ensure(upgraded.is_some(), "weak dispatch upgrades while strong dispatch exists")?;
    drop(upgraded);
    drop(dispatch);
    ensure(
      weak.upgrade().is_none(),
      "weak dispatch stops upgrading after strong dispatch drops",
    )?;

    ensure_eq(
      &counts.register_dispatch.load(Ordering::SeqCst),
      &1,
      "dispatch construction invokes on_register_dispatch",
    )
  }

  #[test]
  fn dispatch_forwards_event_subscriber_hooks() -> Result<(), TestFailure> {
    let (dispatch, counts) = recording_dispatch(true);
    let metadata = EVENT_CALLSITE.metadata();
    let fields = metadata.fields();
    let message = ensure_some(fields.field("message"), "message field exists")?;
    let message_value: &dyn Value = &"dispatched";
    let values = [(&message, Some(message_value))];
    let value_set = fields.value_set(&values);
    let event = Event::new(metadata, &value_set);

    let interest = ensure_ok(dispatch.register_callsite(metadata), "dispatch forwards callsite registration")?;
    ensure(interest.is_sometimes(), "callsite registration returns the subscriber interest")?;
    ensure(
      ensure_ok(dispatch.enabled(metadata), "dispatch forwards enabled")?,
      "subscriber enabled result propagates",
    )?;
    let concrete_subscriber = ensure_some(
      dispatch.downcast_ref::<RecordingSubscriber>(),
      "dispatch downcasts before direct event-enabled checks",
    )?;
    ensure(
      ensure_ok(concrete_subscriber.event_enabled(&event), "subscriber evaluates event_enabled")?,
      "event-enabled result propagates",
    )?;
    ensure_ok(dispatch.event(&event), "dispatch forwards event")?;

    ensure_eq(
      &counts.register_dispatch.load(Ordering::SeqCst),
      &1,
      "dispatch construction invokes on_register_dispatch",
    )?;
    ensure_eq(
      &counts.register_callsite.load(Ordering::SeqCst),
      &1,
      "dispatch forwards register_callsite",
    )?;
    ensure_eq(&counts.enabled.load(Ordering::SeqCst), &1, "dispatch forwards enabled")?;
    ensure_eq(
      &counts.event_enabled.load(Ordering::SeqCst),
      &2,
      "dispatch and direct subscriber checks evaluate event_enabled",
    )?;
    ensure_eq(&counts.event.load(Ordering::SeqCst), &1, "dispatch forwards event")
  }

  #[test]
  fn dispatch_forwards_span_recording_hooks() -> Result<(), TestFailure> {
    let (dispatch, counts) = recording_dispatch(true);
    let span_metadata = SPAN_CALLSITE.metadata();
    let span_values = span_metadata.fields().value_set_all(&[]);
    let attrs = span::Attributes::new(span_metadata, &span_values);
    let span_id = ensure_ok(dispatch.new_span(&attrs), "dispatch forwards new_span")?;
    let fields = EVENT_CALLSITE.metadata().fields();
    let message = ensure_some(fields.field("message"), "message field exists")?;
    let message_value: &dyn Value = &"dispatched";
    let values = [(&message, Some(message_value))];
    let value_set = fields.value_set(&values);
    let record = span::Record::new(&value_set);

    ensure_ok(dispatch.record(span_id, &record), "dispatch forwards record")?;
    ensure_ok(dispatch.record_follows_from(span_id, span_id), "dispatch forwards follows-from")?;

    ensure_eq(
      &counts.register_dispatch.load(Ordering::SeqCst),
      &1,
      "dispatch construction invokes on_register_dispatch",
    )?;
    ensure_eq(&counts.new_span.load(Ordering::SeqCst), &1, "dispatch forwards new_span")?;
    ensure_eq(&counts.record.load(Ordering::SeqCst), &1, "dispatch forwards record")?;
    ensure_eq(
      &counts.follows_from.load(Ordering::SeqCst),
      &1,
      "dispatch forwards record_follows_from",
    )
  }

  #[test]
  fn dispatch_forwards_span_lifecycle_hooks() -> Result<(), TestFailure> {
    let (dispatch, counts) = recording_dispatch(true);
    let span_metadata = SPAN_CALLSITE.metadata();
    let span_values = span_metadata.fields().value_set_all(&[]);
    let attrs = span::Attributes::new(span_metadata, &span_values);
    let span_id = ensure_ok(dispatch.new_span(&attrs), "dispatch forwards new_span")?;

    ensure_ok(dispatch.enter(span_id), "dispatch forwards enter")?;
    ensure_ok(dispatch.exit(span_id), "dispatch forwards exit")?;
    let cloned = ensure_ok(dispatch.clone_span(span_id), "dispatch forwards clone_span")?;
    ensure(cloned == span_id, "clone_span returns the subscriber-provided ID")?;
    ensure(
      ensure_ok(dispatch.try_close(span_id), "dispatch forwards try_close")?,
      "try_close result propagates",
    )?;
    let current = ensure_ok(dispatch.current_span(), "dispatch forwards current_span")?;
    ensure(current.is_known(), "current span is known")?;
    ensure(current.id().is_some(), "current span exposes the active ID")?;
    ensure_eq(
      &ensure_some(current.metadata(), "current span metadata is present")?.name(),
      &"core_contract_span",
      "current span exposes metadata",
    )?;

    ensure_eq(&counts.enter.load(Ordering::SeqCst), &1, "dispatch forwards enter")?;
    ensure_eq(&counts.exit.load(Ordering::SeqCst), &1, "dispatch forwards exit")?;
    ensure_eq(&counts.clone_span.load(Ordering::SeqCst), &1, "dispatch forwards clone_span")?;
    ensure_eq(&counts.try_close.load(Ordering::SeqCst), &1, "dispatch forwards try_close")?;
    ensure_eq(&counts.current_span.load(Ordering::SeqCst), &1, "dispatch forwards current_span")
  }

  #[test]
  fn subscriber_errors_and_trait_object_downcasts_are_stable() -> Result<(), TestFailure> {
    let error = SubscriberError::from_args(format_args!("subscriber failed: {}", "closed"));
    ensure_eq(
      &error.as_str(),
      &"subscriber failed: closed",
      "subscriber errors preserve formatted messages",
    )?;
    ensure_eq(
      &error.to_string(),
      &"subscriber failed: closed".to_owned(),
      "subscriber errors display their messages",
    )?;
    ensure_contains(
      &format!("{error:?}"),
      "subscriber failed: closed",
      "subscriber error debug includes messages",
    )?;
    ensure_contains(
      &SubscriberError::lock_poisoned().to_string(),
      "subscriber synchronization lock is poisoned",
      "lock-poisoned errors use the public synchronization message",
    )?;

    let subscriber = RecordingSubscriber {
      counts:  Arc::new(HookCounts::default()),
      enabled: false,
    };
    let plain: &dyn Subscriber = &subscriber;
    let send: &(dyn Subscriber + Send) = &subscriber;
    let sync: &(dyn Subscriber + Sync) = &subscriber;
    let send_sync: &(dyn Subscriber + Send + Sync) = &subscriber;

    ensure(
      plain.is::<RecordingSubscriber>(),
      "plain trait objects downcast to their concrete type",
    )?;
    ensure(
      send.is::<RecordingSubscriber>(),
      "send trait objects downcast to their concrete type",
    )?;
    ensure(
      sync.is::<RecordingSubscriber>(),
      "sync trait objects downcast to their concrete type",
    )?;
    ensure(
      send_sync.is::<RecordingSubscriber>(),
      "send-sync trait objects downcast to their concrete type",
    )?;
    ensure(
      plain.downcast_ref::<NoOpSubscriber<Primary>>().is_none(),
      "plain trait objects reject unrelated downcasts",
    )
  }
}
