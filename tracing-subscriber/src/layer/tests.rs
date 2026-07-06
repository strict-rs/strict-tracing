use alloc::format;
use alloc::sync::Arc;
use alloc::vec;
use core::num::NonZeroU64;

use parking_lot::Mutex;
use strict_test_support::TestFailure;
use strict_test_support::ensure;
use strict_test_support::ensure_contains;
use strict_test_support::ensure_ok;
use strict_test_support::ensure_some;
use tracing::subscriber;
use tracing_core::Level;
use tracing_core::callsite::Callsite;
use tracing_core::metadata::Kind;
use tracing_core::subscriber::NoSubscriber;
use tracing_core::subscriber::SubscriberResult;

use super::*;

/// Callsite used by layer wrapper metadata fixtures.
struct LayerTestCallsite;

/// Shared callsite used by layer wrapper metadata fixtures.
static LAYER_TEST_CALLSITE: LayerTestCallsite = LayerTestCallsite;

/// Metadata used by layer wrapper tests.
static LAYER_TEST_META: Metadata<'static> = tracing_core::metadata! {
  name: "layer_test",
  target: "layer_target",
  level: Level::INFO,
  fields: &["answer"],
  callsite: &LAYER_TEST_CALLSITE,
  kind: Kind::EVENT,
};

impl Callsite for LayerTestCallsite {
  fn set_interest(&self, _: Interest) {}

  fn metadata(&self) -> &Metadata<'_> {
    &LAYER_TEST_META
  }
}

/// Shared hook call log for layer wrapper tests.
type CallLog = Arc<Mutex<Vec<(&'static str, &'static str)>>>;

/// Layer whose behavior and hook log are controlled by tests.
#[derive(Debug)]
struct RecordingLayer {
  /// Layer label written to the hook log.
  label:         &'static str,
  /// Return value for [`Layer::register_callsite`].
  interest:      Interest,
  /// Return value for [`Layer::enabled`].
  enabled:       bool,
  /// Return value for [`Layer::event_enabled`].
  event_enabled: bool,
  /// Return value for [`Layer::max_level_hint`].
  max_level:     Option<LevelFilter>,
  /// Shared hook call log.
  calls:         CallLog,
}

impl RecordingLayer {
  /// Records a hook invocation.
  fn record(&self, hook: &'static str) {
    self.calls.lock().push((self.label, hook));
  }
}

impl<S: Subscriber> Layer<S> for RecordingLayer {
  fn on_layer(&mut self, _subscriber: &mut S) {
    self.record("on_layer");
  }

  fn on_register_dispatch(&self, _subscriber: &Dispatch) -> SubscriberResult<()> {
    self.record("on_register_dispatch");
    Ok(())
  }

  fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
    self.record("register_callsite");
    Ok(self.interest)
  }

  fn enabled(&self, _metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> SubscriberResult<bool> {
    self.record("enabled");
    Ok(self.enabled)
  }

  fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> SubscriberResult<bool> {
    self.record("event_enabled");
    Ok(self.event_enabled)
  }

  fn on_new_span(&self, _attrs: &span::Attributes<'_>, _id: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_new_span");
    Ok(())
  }

  fn on_record(&self, _span: span::Id, _values: &span::Record<'_>, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_record");
    Ok(())
  }

  fn on_follows_from(&self, _span: span::Id, _follows: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_follows_from");
    Ok(())
  }

  fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_event");
    Ok(())
  }

  fn on_enter(&self, _id: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_enter");
    Ok(())
  }

  fn on_exit(&self, _id: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_exit");
    Ok(())
  }

  fn on_close(&self, _id: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_close");
    Ok(())
  }

  fn on_id_change(&self, _old: span::Id, _new: span::Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
    self.record("on_id_change");
    Ok(())
  }

  fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
    self.record("max_level_hint");
    Ok(self.max_level)
  }
}

/// Subscriber whose hook behavior and hook log are controlled by tests.
struct RecordingSubscriber {
  /// Return value for [`Subscriber::register_callsite`].
  interest:      Interest,
  /// Return value for [`Subscriber::enabled`].
  enabled:       bool,
  /// Return value for [`Subscriber::event_enabled`].
  event_enabled: bool,
  /// Return value for [`Subscriber::max_level_hint`].
  max_level:     Option<LevelFilter>,
  /// Span ID returned by [`Subscriber::new_span`].
  new_id:        span::Id,
  /// Span ID returned by [`Subscriber::clone_span`].
  clone_id:      span::Id,
  /// Return value for [`Subscriber::try_close`].
  closed_span:   Option<span::Id>,
  /// Shared hook call log.
  calls:         CallLog,
}

impl RecordingSubscriber {
  /// Records a hook invocation.
  fn record_hook(&self, hook: &'static str) {
    self.calls.lock().push(("inner", hook));
  }
}

impl Subscriber for RecordingSubscriber {
  fn on_register_dispatch(&self, _subscriber: &Dispatch) -> SubscriberResult {
    self.record_hook("on_register_dispatch");
    Ok(())
  }

  fn register_callsite(&self, _: &'static Metadata<'static>) -> SubscriberResult<Interest> {
    self.record_hook("register_callsite");
    Ok(self.interest)
  }

  fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
    self.record_hook("enabled");
    Ok(self.enabled)
  }

  fn event_enabled(&self, _: &Event<'_>) -> SubscriberResult<bool> {
    self.record_hook("event_enabled");
    Ok(self.event_enabled)
  }

  fn max_level_hint(&self) -> Option<LevelFilter> {
    self.record_hook("max_level_hint");
    self.max_level
  }

  fn new_span(&self, _: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
    self.record_hook("new_span");
    Ok(self.new_id)
  }

  fn record(&self, _: span::Id, _: &span::Record<'_>) -> SubscriberResult {
    self.record_hook("record");
    Ok(())
  }

  fn record_follows_from(&self, _: span::Id, _: span::Id) -> SubscriberResult {
    self.record_hook("record_follows_from");
    Ok(())
  }

  fn event(&self, _: &Event<'_>) -> SubscriberResult {
    self.record_hook("event");
    Ok(())
  }

  fn enter(&self, _: span::Id) -> SubscriberResult {
    self.record_hook("enter");
    Ok(())
  }

  fn exit(&self, _: span::Id) -> SubscriberResult {
    self.record_hook("exit");
    Ok(())
  }

  fn clone_span(&self, _: span::Id) -> SubscriberResult<span::Id> {
    self.record_hook("clone_span");
    Ok(self.clone_id)
  }

  fn try_close(&self, _: span::Id) -> SubscriberResult<bool> {
    self.record_hook("try_close");
    Ok(self.closed_span.is_some())
  }

  fn current_span(&self) -> SubscriberResult<span::Current> {
    self.record_hook("current_span");
    Ok(span::Current::none())
  }
}

/// Runs a callback with event/span fixtures for layer wrapper tests.
fn with_layer_fixtures<R>(
  f: impl FnOnce(&Event<'_>, &span::Attributes<'_>, &span::Record<'_>, span::Id, span::Id) -> Result<R, TestFailure>,
) -> Result<R, TestFailure> {
  let values = LAYER_TEST_META.fields().value_set_all(&[]);
  let event = Event::new(&LAYER_TEST_META, &values);
  let attrs = span::Attributes::new(&LAYER_TEST_META, &values);
  let record = span::Record::new(&values);
  let first_id = span::Id::from_non_zero_u64(NonZeroU64::MIN);
  let second_id = span::Id::from_non_zero_u64(NonZeroU64::MAX);
  f(&event, &attrs, &record, first_id, second_id)
}

/// Returns a recording layer with common defaults.
fn recording_layer(label: &'static str, calls: &CallLog) -> RecordingLayer {
  RecordingLayer {
    label,
    interest: Interest::always(),
    enabled: true,
    event_enabled: true,
    max_level: Some(LevelFilter::INFO),
    calls: Arc::clone(calls),
  }
}

/// Returns a recording subscriber with common defaults.
fn recording_subscriber(calls: &CallLog, new_id: span::Id, clone_id: span::Id) -> RecordingSubscriber {
  RecordingSubscriber {
    interest: Interest::always(),
    enabled: true,
    event_enabled: true,
    max_level: Some(LevelFilter::DEBUG),
    new_id,
    clone_id,
    closed_span: Some(new_id),
    calls: Arc::clone(calls),
  }
}

#[derive(Debug)]
struct NopLayer;
impl<S: Subscriber> Layer<S> for NopLayer {}

struct NopLayer2;
impl<S: Subscriber> Layer<S> for NopLayer2 {}

/// A layer that holds a string.
///
/// Used to test that pointers returned by downcasting are actually valid.
struct StringLayer(&'static str);
impl<S: Subscriber> Layer<S> for StringLayer {}
struct StringLayer2(&'static str);
impl<S: Subscriber> Layer<S> for StringLayer2 {}

struct StringLayer3(&'static str);
impl<S: Subscriber> Layer<S> for StringLayer3 {}

struct StringSubscriber(&'static str);

impl Subscriber for StringSubscriber {
  fn register_callsite(&self, _: &'static Metadata<'static>) -> SubscriberResult<Interest> {
    Ok(Interest::never())
  }

  fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(false)
  }

  fn new_span(&self, _: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
    Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
  }

  fn record(&self, _: span::Id, _: &span::Record<'_>) -> SubscriberResult {
    Ok(())
  }

  fn record_follows_from(&self, _: span::Id, _: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn event(&self, _: &Event<'_>) -> SubscriberResult {
    Ok(())
  }

  fn enter(&self, _: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn exit(&self, _: span::Id) -> SubscriberResult {
    Ok(())
  }
}

fn assert_subscriber(_s: impl Subscriber) {}
fn assert_layer<S: Subscriber>(_l: &impl Layer<S>) {}

#[test]
fn layer_is_subscriber() {
  let subscriber = NopLayer.with_subscriber(NoSubscriber::default());
  assert_subscriber(subscriber);
}

#[test]
fn two_layers_are_subscriber() {
  let subscriber = NopLayer.and_then(NopLayer2).with_subscriber(NoSubscriber::default());
  assert_subscriber(subscriber);
}

#[test]
fn three_layers_are_subscriber() {
  let subscriber = NopLayer
    .and_then(NopLayer)
    .and_then(NopLayer)
    .with_subscriber(NoSubscriber::default());
  assert_subscriber(subscriber);
}

#[test]
fn three_layers_are_layer() {
  let layers = NopLayer.and_then(NopLayer).and_then(NopLayer);
  assert_layer(&layers);
  let _layered = layers.with_subscriber(NoSubscriber::default());
}

#[test]
#[cfg(feature = "alloc")]
fn box_layer_is_layer() {
  use alloc::boxed::Box;
  let layer: Box<dyn Layer<NoSubscriber> + Send + Sync> = Box::new(NopLayer);
  assert_layer(&layer);
  let _layered = layer.with_subscriber(NoSubscriber::default());
}

#[test]
fn downcasts_to_subscriber() -> Result<(), TestFailure> {
  let subscriber = NopLayer
    .and_then(NopLayer)
    .and_then(NopLayer)
    .with_subscriber(StringSubscriber("subscriber"));
  let downcast_subscriber = ensure_some(
    <dyn Subscriber>::downcast_ref::<StringSubscriber>(&subscriber),
    "subscriber should downcast",
  )?;
  ensure(downcast_subscriber.0 == "subscriber", "downcast subscriber preserves inner value")
}

#[test]
fn downcasts_to_layer() -> Result<(), TestFailure> {
  let subscriber = StringLayer("layer_1")
    .and_then(StringLayer2("layer_2"))
    .and_then(StringLayer3("layer_3"))
    .with_subscriber(NoSubscriber::default());
  let first_layer = ensure_some(
    <dyn Subscriber>::downcast_ref::<StringLayer>(&subscriber),
    "layer 1 should downcast",
  )?;
  ensure(first_layer.0 == "layer_1", "first layer downcasts")?;
  let second_layer = ensure_some(
    <dyn Subscriber>::downcast_ref::<StringLayer2>(&subscriber),
    "layer 2 should downcast",
  )?;
  ensure(second_layer.0 == "layer_2", "second layer downcasts")?;
  let third_layer = ensure_some(
    <dyn Subscriber>::downcast_ref::<StringLayer3>(&subscriber),
    "layer 3 should downcast",
  )?;
  ensure(third_layer.0 == "layer_3", "third layer downcasts")
}

#[test]
fn identity_and_optional_layers_expose_absence_contracts() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, record, first_id, second_id| {
    let identity = Identity::new();
    ensure(
      !layer_is_none::<_, NoSubscriber>(&identity),
      "identity is not an absent optional layer",
    )?;

    let none_layer: Option<RecordingLayer> = None;
    let context = Context::<NoSubscriber>::none();
    ensure(
      layer_is_none::<_, NoSubscriber>(&none_layer),
      "none optional layer exposes the absence marker",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::register_callsite(&none_layer, &LAYER_TEST_META),
        "none layer interest",
      )?
      .is_always(),
      "none optional layer keeps callsites globally enabled",
    )?;
    ensure(
      ensure_ok(none_layer.enabled(&LAYER_TEST_META, context.clone()), "none layer enabled")?,
      "none optional layer keeps metadata globally enabled",
    )?;
    ensure(
      ensure_ok(none_layer.event_enabled(event, context.clone()), "none layer event enabled")?,
      "none optional layer keeps events globally enabled",
    )?;
    ensure(
      ensure_ok(Layer::<NoSubscriber>::max_level_hint(&none_layer), "none layer max level")? == Some(LevelFilter::OFF),
      "none optional layer reports an OFF max-level hint",
    )?;
    ensure_ok(none_layer.on_new_span(attrs, first_id, context.clone()), "none layer new span")?;
    ensure_ok(none_layer.on_record(first_id, record, context.clone()), "none layer record")?;
    ensure_ok(
      none_layer.on_follows_from(first_id, second_id, context.clone()),
      "none layer follows-from",
    )?;
    ensure_ok(none_layer.on_event(event, context.clone()), "none layer event")?;
    ensure_ok(none_layer.on_enter(first_id, context.clone()), "none layer enter")?;
    ensure_ok(none_layer.on_exit(first_id, context.clone()), "none layer exit")?;
    ensure_ok(none_layer.on_close(first_id, context.clone()), "none layer close")?;
    ensure_ok(none_layer.on_id_change(first_id, second_id, context), "none layer ID change")?;

    let calls = Arc::new(Mutex::new(Vec::new()));
    let some_layer = Some(recording_layer("some", &calls));
    ensure(
      !layer_is_none::<_, NoSubscriber>(&some_layer),
      "present optional layer is not absent",
    )?;
    ensure(
      Layer::<NoSubscriber>::downcast_ref_by_id(&some_layer, TypeId::of::<RecordingLayer>()).is_some(),
      "present optional layer forwards downcasts",
    )?;
    ensure(
      Layer::<NoSubscriber>::downcast_ref_by_id(&some_layer, TypeId::of::<NoneLayerMarker>()).is_none(),
      "present optional layer does not expose the absence marker",
    )
  })
}

#[test]
fn present_optional_layers_forward_hooks_to_inner_layer() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, record, first_id, second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut layer = Some(recording_layer("some", &calls));
    let dispatch = Dispatch::new(NoSubscriber::default());
    let mut subscriber = NoSubscriber::default();
    let context = Context::<NoSubscriber>::none();

    layer.on_layer(&mut subscriber);
    let layer_ref: &dyn Layer<NoSubscriber> = &layer;
    ensure_ok(layer_ref.on_register_dispatch(&dispatch), "some option layer dispatch")?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::register_callsite(&layer, &LAYER_TEST_META),
        "some option layer interest",
      )?
      .is_always(),
      "some option layer forwards callsite interest",
    )?;
    ensure(
      ensure_ok(layer.enabled(&LAYER_TEST_META, context.clone()), "some option layer enabled")?,
      "some option layer forwards enabled",
    )?;
    ensure(
      ensure_ok(layer.event_enabled(event, context.clone()), "some option layer event enabled")?,
      "some option layer forwards event_enabled",
    )?;
    ensure(
      ensure_ok(Layer::<NoSubscriber>::max_level_hint(&layer), "some option layer max level")? == Some(LevelFilter::INFO),
      "some option layer forwards max-level hint",
    )?;
    ensure_ok(layer.on_new_span(attrs, first_id, context.clone()), "some option layer new span")?;
    ensure_ok(layer.on_record(first_id, record, context.clone()), "some option layer record")?;
    ensure_ok(
      layer.on_follows_from(first_id, second_id, context.clone()),
      "some option layer follows-from",
    )?;
    ensure_ok(layer.on_event(event, context.clone()), "some option layer event")?;
    ensure_ok(layer.on_enter(first_id, context.clone()), "some option layer enter")?;
    ensure_ok(layer.on_exit(first_id, context.clone()), "some option layer exit")?;
    ensure_ok(layer.on_close(first_id, context.clone()), "some option layer close")?;
    ensure_ok(layer.on_id_change(first_id, second_id, context), "some option layer ID change")?;

    let expected = vec![
      ("some", "on_layer"),
      ("some", "on_register_dispatch"),
      ("some", "register_callsite"),
      ("some", "enabled"),
      ("some", "event_enabled"),
      ("some", "max_level_hint"),
      ("some", "on_new_span"),
      ("some", "on_record"),
      ("some", "on_follows_from"),
      ("some", "on_event"),
      ("some", "on_enter"),
      ("some", "on_exit"),
      ("some", "on_close"),
      ("some", "on_id_change"),
    ];
    ensure(*calls.lock() == expected, "present optional layers forward every hook")
  })
}

#[test]
fn vec_layers_combine_filtering_and_fan_out_lifecycle_hooks() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, record, first_id, second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let first = recording_layer("first", &calls);
    let mut second = recording_layer("second", &calls);
    second.interest = Interest::sometimes();
    second.enabled = false;
    second.event_enabled = false;
    second.max_level = Some(LevelFilter::DEBUG);
    let third = recording_layer("third", &calls);
    let layers = vec![first, second, third];
    let context = Context::<NoSubscriber>::none();

    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::register_callsite(&layers, &LAYER_TEST_META),
        "vec layer interest",
      )?
      .is_always(),
      "vec layers promote callsite interest to the highest interest",
    )?;
    ensure(
      !ensure_ok(layers.enabled(&LAYER_TEST_META, context.clone()), "vec layer enabled")?,
      "vec layers short-circuit metadata filtering on the first false layer",
    )?;
    ensure(
      !ensure_ok(layers.event_enabled(event, context.clone()), "vec layer event enabled")?,
      "vec layers short-circuit event filtering on the first false layer",
    )?;
    ensure(
      ensure_ok(Layer::<NoSubscriber>::max_level_hint(&layers), "vec layer max level")? == Some(LevelFilter::DEBUG),
      "vec layers report the most verbose concrete max-level hint",
    )?;
    ensure_ok(layers.on_new_span(attrs, first_id, context.clone()), "vec layer new span")?;
    ensure_ok(layers.on_record(first_id, record, context.clone()), "vec layer record")?;
    ensure_ok(
      layers.on_follows_from(first_id, second_id, context.clone()),
      "vec layer follows-from",
    )?;
    ensure_ok(layers.on_event(event, context.clone()), "vec layer event")?;
    ensure_ok(layers.on_enter(first_id, context.clone()), "vec layer enter")?;
    ensure_ok(layers.on_exit(first_id, context.clone()), "vec layer exit")?;
    ensure_ok(layers.on_close(first_id, context.clone()), "vec layer close")?;

    let expected = vec![
      ("first", "register_callsite"),
      ("second", "register_callsite"),
      ("third", "register_callsite"),
      ("first", "enabled"),
      ("second", "enabled"),
      ("first", "event_enabled"),
      ("second", "event_enabled"),
      ("first", "max_level_hint"),
      ("second", "max_level_hint"),
      ("third", "max_level_hint"),
      ("first", "on_new_span"),
      ("second", "on_new_span"),
      ("third", "on_new_span"),
      ("first", "on_record"),
      ("second", "on_record"),
      ("third", "on_record"),
      ("first", "on_follows_from"),
      ("second", "on_follows_from"),
      ("third", "on_follows_from"),
      ("first", "on_event"),
      ("second", "on_event"),
      ("third", "on_event"),
      ("first", "on_enter"),
      ("second", "on_enter"),
      ("third", "on_enter"),
      ("first", "on_exit"),
      ("second", "on_exit"),
      ("third", "on_exit"),
      ("first", "on_close"),
      ("second", "on_close"),
      ("third", "on_close"),
    ];
    ensure(*calls.lock() == expected, "vec layers preserve short-circuit and fan-out order")
  })
}

#[test]
fn vec_layers_report_off_for_empty_and_none_for_unhinted_max_levels() -> Result<(), TestFailure> {
  let empty: Vec<RecordingLayer> = Vec::new();
  ensure(
    ensure_ok(Layer::<NoSubscriber>::max_level_hint(&empty), "empty vec layer max level")? == Some(LevelFilter::OFF),
    "empty vec layers report OFF as their max-level hint",
  )?;
  let unhinted_calls = Arc::new(Mutex::new(Vec::new()));
  let mut unhinted = recording_layer("unhinted", &unhinted_calls);
  unhinted.max_level = None;
  let unhinted_layers = vec![unhinted];
  ensure(
    ensure_ok(
      Layer::<NoSubscriber>::max_level_hint(&unhinted_layers),
      "unhinted vec layer max level",
    )?
    .is_none(),
    "vec layers return no hint when any inner layer cannot provide one",
  )
}

#[test]
#[cfg(any(feature = "alloc", feature = "std"))]
fn boxed_layers_forward_downcasts_and_debug_contracts() -> Result<(), TestFailure> {
  let boxed: Box<dyn Layer<NoSubscriber> + Send + Sync> = Box::new(StringLayer("boxed"));
  let downcast = ensure_some(
    boxed
      .downcast_ref_by_id(TypeId::of::<StringLayer>())
      .and_then(<dyn Any>::downcast_ref::<StringLayer>),
    "boxed layer forwards downcasts to the inner layer",
  )?;
  ensure(downcast.0 == "boxed", "boxed layer downcast preserves the inner value")?;

  let boxed_debug = format!("{:?}", Identity::new());
  ensure_contains(&boxed_debug, "Identity", "identity debug names the layer")?;

  let calls = Arc::new(Mutex::new(Vec::new()));
  let boxed_recording: Box<dyn Layer<NoSubscriber> + Send + Sync> = recording_layer("boxed", &calls).boxed();
  ensure(
    boxed_recording.downcast_ref_by_id(TypeId::of::<RecordingLayer>()).is_some(),
    "boxed method erases type while preserving downcast access",
  )
}

#[test]
fn default_layer_hooks_are_noops_and_leave_filtering_enabled() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, record, first_id, second_id| {
    let layer = NopLayer;
    let dispatch = Dispatch::none();
    let context = Context::<NoSubscriber>::none();

    ensure_ok(
      Layer::<NoSubscriber>::on_register_dispatch(&layer, &dispatch),
      "default layer dispatch registration is a no-op",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::register_callsite(&layer, &LAYER_TEST_META),
        "default layer callsite registration",
      )?
      .is_always(),
      "default layer callsite registration enables metadata",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::enabled(&layer, &LAYER_TEST_META, context.clone()),
        "default layer metadata enabled",
      )?,
      "default layer enables metadata",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::event_enabled(&layer, event, context.clone()),
        "default layer event enabled",
      )?,
      "default layer enables events",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_new_span(&layer, attrs, first_id, context.clone()),
      "default layer new-span notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_record(&layer, first_id, record, context.clone()),
      "default layer record notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_follows_from(&layer, first_id, second_id, context.clone()),
      "default layer follows-from notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_event(&layer, event, context.clone()),
      "default layer event notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_enter(&layer, first_id, context.clone()),
      "default layer enter notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_exit(&layer, first_id, context.clone()),
      "default layer exit notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_close(&layer, first_id, context.clone()),
      "default layer close notification is a no-op",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_id_change(&layer, first_id, second_id, context),
      "default layer ID-change notification is a no-op",
    )?;
    ensure(
      ensure_ok(Layer::<NoSubscriber>::max_level_hint(&layer), "default layer max-level hint")?.is_none(),
      "default layer reports no max-level hint",
    )
  })
}

#[test]
fn layered_subscriber_exposes_downcasts_and_query_hooks() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, _record, first_id, second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let layer = recording_layer("outer", &calls);
    let inner = recording_subscriber(&calls, first_id, second_id);
    let layered = layer.with_subscriber(inner);
    let dispatch = Dispatch::none();

    ensure(layered.is::<RecordingLayer>(), "layered subscriber exposes layer downcasts")?;
    ensure(
      layered.downcast_ref::<RecordingSubscriber>().is_some(),
      "layered subscriber exposes inner subscriber downcasts",
    )?;
    ensure_ok(
      Subscriber::on_register_dispatch(&layered, &dispatch),
      "layered subscriber forwards dispatch registration",
    )?;
    ensure(
      ensure_ok(
        Subscriber::register_callsite(&layered, &LAYER_TEST_META),
        "layered subscriber callsite registration",
      )?
      .is_always(),
      "layered subscriber combines callsite interest",
    )?;
    ensure(
      ensure_ok(Subscriber::enabled(&layered, &LAYER_TEST_META), "layered subscriber enabled")?,
      "layered subscriber combines enabled checks",
    )?;
    ensure(
      Subscriber::max_level_hint(&layered) == Some(LevelFilter::DEBUG),
      "layered subscriber combines max-level hints",
    )?;
    let new_id = ensure_ok(Subscriber::new_span(&layered, attrs), "layered subscriber new span")?;
    ensure(new_id == first_id, "layered subscriber returns inner span ID")?;
    ensure(
      ensure_ok(Subscriber::event_enabled(&layered, event), "layered subscriber event enabled")?,
      "layered subscriber combines event-enabled checks",
    )?;
    let current = ensure_ok(Subscriber::current_span(&layered), "layered subscriber current span")?;
    ensure(current.id().is_none(), "layered subscriber forwards current span state")?;

    let expected = vec![
      ("outer", "on_layer"),
      ("inner", "on_register_dispatch"),
      ("outer", "on_register_dispatch"),
      ("outer", "register_callsite"),
      ("inner", "register_callsite"),
      ("outer", "enabled"),
      ("inner", "enabled"),
      ("outer", "max_level_hint"),
      ("inner", "max_level_hint"),
      ("inner", "new_span"),
      ("outer", "on_new_span"),
      ("outer", "event_enabled"),
      ("inner", "event_enabled"),
      ("inner", "current_span"),
    ];
    ensure(*calls.lock() == expected, "layered subscriber preserves query hook ordering")
  })
}

#[test]
fn layered_subscriber_forwards_record_event_and_lifecycle_hooks() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, _attrs, record, first_id, second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let layer = recording_layer("outer", &calls);
    let inner = recording_subscriber(&calls, first_id, second_id);
    let layered = layer.with_subscriber(inner);

    ensure_ok(Subscriber::record(&layered, first_id, record), "layered subscriber record")?;
    ensure_ok(
      Subscriber::record_follows_from(&layered, first_id, second_id),
      "layered subscriber follows-from",
    )?;
    ensure_ok(Subscriber::event(&layered, event), "layered subscriber event")?;
    ensure_ok(Subscriber::enter(&layered, first_id), "layered subscriber enter")?;
    ensure_ok(Subscriber::exit(&layered, first_id), "layered subscriber exit")?;
    let cloned = ensure_ok(Subscriber::clone_span(&layered, first_id), "layered subscriber clone")?;
    ensure(cloned == second_id, "layered subscriber returns cloned span ID")?;
    ensure(
      ensure_ok(Subscriber::try_close(&layered, first_id), "layered subscriber close")?,
      "layered subscriber returns inner close result",
    )?;

    let expected = vec![
      ("outer", "on_layer"),
      ("inner", "record"),
      ("outer", "on_record"),
      ("inner", "record_follows_from"),
      ("outer", "on_follows_from"),
      ("inner", "event"),
      ("outer", "on_event"),
      ("inner", "enter"),
      ("outer", "on_enter"),
      ("inner", "exit"),
      ("outer", "on_exit"),
      ("inner", "clone_span"),
      ("outer", "on_id_change"),
      ("inner", "try_close"),
      ("outer", "on_close"),
    ];
    ensure(*calls.lock() == expected, "layered subscriber preserves lifecycle hook ordering")
  })
}

#[test]
fn layered_layer_forwards_query_hooks_in_order() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, _attrs, _record, _first_id, _second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut inner = recording_layer("inner", &calls);
    inner.interest = Interest::sometimes();
    inner.max_level = Some(LevelFilter::DEBUG);
    let outer = recording_layer("outer", &calls);
    let mut layered = inner.and_then(outer);
    let dispatch = Dispatch::none();
    let context = Context::<NoSubscriber>::none();
    let mut subscriber = NoSubscriber::default();

    layered.on_layer(&mut subscriber);
    ensure_ok(
      Layer::<NoSubscriber>::on_register_dispatch(&layered, &dispatch),
      "layered layer dispatch registration",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::register_callsite(&layered, &LAYER_TEST_META),
        "layered layer callsite registration",
      )?
      .is_sometimes(),
      "layered layer combines callsite interest from the inner layer",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::enabled(&layered, &LAYER_TEST_META, context.clone()),
        "layered layer enabled",
      )?,
      "layered layer asks both layers when the outer layer enables metadata",
    )?;
    ensure(
      ensure_ok(Layer::<NoSubscriber>::max_level_hint(&layered), "layered layer max level")? == Some(LevelFilter::DEBUG),
      "layered layer chooses the most verbose max-level hint",
    )?;
    ensure(
      ensure_ok(
        Layer::<NoSubscriber>::event_enabled(&layered, event, context.clone()),
        "layered layer event enabled",
      )?,
      "layered layer asks both layers when the outer layer enables events",
    )?;
    ensure(
      Layer::<NoSubscriber>::downcast_ref_by_id(&layered, TypeId::of::<RecordingLayer>()).is_some(),
      "layered layer exposes child layer downcasts",
    )?;

    let expected = vec![
      ("outer", "on_layer"),
      ("inner", "on_layer"),
      ("outer", "on_register_dispatch"),
      ("inner", "on_register_dispatch"),
      ("outer", "register_callsite"),
      ("inner", "register_callsite"),
      ("outer", "enabled"),
      ("inner", "enabled"),
      ("outer", "max_level_hint"),
      ("inner", "max_level_hint"),
      ("outer", "event_enabled"),
      ("inner", "event_enabled"),
    ];
    ensure(*calls.lock() == expected, "layered layer preserves query hook ordering")
  })
}

#[test]
fn layered_layer_forwards_lifecycle_hooks_in_order() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, attrs, record, first_id, second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = recording_layer("inner", &calls);
    let outer = recording_layer("outer", &calls);
    let layered = inner.and_then(outer);
    let context = Context::<NoSubscriber>::none();

    ensure_ok(
      Layer::<NoSubscriber>::on_new_span(&layered, attrs, first_id, context.clone()),
      "layered layer new span",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_record(&layered, first_id, record, context.clone()),
      "layered layer record",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_follows_from(&layered, first_id, second_id, context.clone()),
      "layered layer follows-from",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_event(&layered, event, context.clone()),
      "layered layer event",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_enter(&layered, first_id, context.clone()),
      "layered layer enter",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_exit(&layered, first_id, context.clone()),
      "layered layer exit",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_close(&layered, first_id, context.clone()),
      "layered layer close",
    )?;
    ensure_ok(
      Layer::<NoSubscriber>::on_id_change(&layered, first_id, second_id, context),
      "layered layer id change",
    )?;

    let expected = vec![
      ("inner", "on_new_span"),
      ("outer", "on_new_span"),
      ("inner", "on_record"),
      ("outer", "on_record"),
      ("inner", "on_follows_from"),
      ("outer", "on_follows_from"),
      ("inner", "on_event"),
      ("outer", "on_event"),
      ("inner", "on_enter"),
      ("outer", "on_enter"),
      ("inner", "on_exit"),
      ("outer", "on_exit"),
      ("inner", "on_close"),
      ("outer", "on_close"),
      ("inner", "on_id_change"),
      ("outer", "on_id_change"),
    ];
    ensure(*calls.lock() == expected, "layered layer preserves lifecycle hook ordering")
  })
}

#[test]
fn layered_layer_short_circuits_outer_filters() -> Result<(), TestFailure> {
  with_layer_fixtures(|event, _attrs, _record, _first_id, _second_id| {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let inner = recording_layer("inner", &calls);
    let mut outer = recording_layer("outer", &calls);
    outer.enabled = false;
    outer.event_enabled = false;
    let blocked = inner.and_then(outer);
    let context = Context::<NoSubscriber>::none();

    ensure(
      !ensure_ok(
        Layer::<NoSubscriber>::enabled(&blocked, &LAYER_TEST_META, context.clone()),
        "blocked layered layer enabled",
      )?,
      "outer layer can short-circuit metadata enabled checks",
    )?;
    ensure(
      !ensure_ok(
        Layer::<NoSubscriber>::event_enabled(&blocked, event, context),
        "blocked layered layer event enabled",
      )?,
      "outer layer can short-circuit event enabled checks",
    )?;

    let expected = vec![("outer", "enabled"), ("outer", "event_enabled")];
    ensure(
      *calls.lock() == expected,
      "short-circuiting outer layer does not call inner filters",
    )
  })
}

#[cfg(all(feature = "registry", feature = "std"))]
mod registry_tests {
  use super::*;
  use crate::registry::LookupSpan;
  use crate::registry::Registry;

  #[test]
  fn context_event_span() -> Result<(), TestFailure> {
    use std::sync::Arc;

    use parking_lot::Mutex;

    struct RecordingLayer {
      last_event_span: Arc<Mutex<Option<&'static str>>>,
    }

    impl<S> Layer<S> for RecordingLayer
    where
      S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
      fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
        let span = ctx.event_span(event);
        *self.last_event_span.lock() = span.map(|event_span| event_span.name());
        Ok(())
      }
    }

    let last_event_span = Arc::new(Mutex::new(None));

    subscriber::with_default(
      crate::registry().with(RecordingLayer {
        last_event_span: Arc::clone(&last_event_span),
      }),
      || {
        tracing::info!("no span");
        ensure(last_event_span.lock().is_none(), "event outside a span has no event span")?;

        let parent = tracing::info_span!("explicit");
        tracing::info!(parent: &parent, "explicit span");
        ensure(*last_event_span.lock() == Some("explicit"), "explicit parent is event span")?;

        let _guard = tracing::info_span!("contextual").entered();
        tracing::info!("contextual span");
        ensure(*last_event_span.lock() == Some("contextual"), "entered span is event span")
      },
    )
  }

  #[test]
  fn empty_context_reports_absent_span_state_without_blocking_metadata() -> Result<(), TestFailure> {
    let context = Context::<Registry>::none();
    let values = LAYER_TEST_META.fields().value_set_all(&[]);
    let event = Event::new(&LAYER_TEST_META, &values);
    let missing_id = span::Id::from_non_zero_u64(NonZeroU64::MIN);

    ensure(context.current_span().id().is_none(), "empty context has no current span")?;
    ensure(
      context.enabled(&LAYER_TEST_META),
      "empty context does not disable metadata during callsite registration",
    )?;
    context.event(&event);
    ensure(context.event_span(&event).is_none(), "empty context has no contextual event span")?;
    ensure(context.span(missing_id).is_none(), "empty context cannot look up span IDs")?;
    ensure(context.metadata(missing_id).is_none(), "empty context has no span metadata")?;
    ensure(!context.exists(missing_id), "empty context reports missing spans as absent")?;
    ensure(context.lookup_current().is_none(), "empty context has no lookup-current span")?;
    ensure(context.span_scope(missing_id).is_none(), "empty context has no span scope")?;
    ensure(context.event_scope(&event).is_none(), "empty context has no event scope")
  }

  #[test]
  fn registry_context_reports_current_metadata_existence_and_event_scopes() -> Result<(), TestFailure> {
    let registry = Registry::default();
    let context = Context::new(&registry);
    let values = LAYER_TEST_META.fields().value_set_all(&[]);
    let root_attrs = span::Attributes::new_root(&LAYER_TEST_META, &values);
    let root_id = ensure_ok(registry.new_span(&root_attrs), "registry creates root span")?;
    let child_attrs = span::Attributes::child_of(root_id, &LAYER_TEST_META, &values);
    let child_id = ensure_ok(registry.new_span(&child_attrs), "registry creates child span")?;

    ensure(context.exists(root_id), "context finds root span")?;
    ensure(context.exists(child_id), "context finds child span")?;
    ensure(
      context
        .metadata(root_id)
        .is_some_and(|metadata| metadata.name() == "layer_test"),
      "context metadata returns span metadata",
    )?;
    ensure(
      context.span(root_id).is_some_and(|span_ref| span_ref.name() == "layer_test"),
      "context span lookup returns span refs",
    )?;

    let missing_id = span::Id::from_non_zero_u64(NonZeroU64::MAX);
    ensure(!context.exists(missing_id), "context rejects unknown span IDs")?;
    ensure(
      context.metadata(missing_id).is_none(),
      "context returns no metadata for unknown spans",
    )?;

    ensure_ok(registry.enter(child_id), "registry enters child span")?;
    let current = context.current_span();
    ensure(
      current.id().copied() == Some(child_id),
      "context current span mirrors registry current span",
    )?;
    ensure(
      context.lookup_current().is_some_and(|span_ref| span_ref.id() == child_id),
      "context lookup_current returns the current span ref",
    )?;

    let contextual_event = Event::new(&LAYER_TEST_META, &values);
    ensure(
      context
        .event_span(&contextual_event)
        .is_some_and(|span_ref| span_ref.id() == child_id),
      "contextual event uses current span",
    )?;

    let explicit_event = Event::new_child_of(root_id, &LAYER_TEST_META, &values);
    ensure(
      context
        .event_span(&explicit_event)
        .is_some_and(|span_ref| span_ref.id() == root_id),
      "explicit event uses its explicit parent",
    )?;

    let root_event = Event::new_child_of(Option::<span::Id>::None, &LAYER_TEST_META, &values);
    ensure(context.event_span(&root_event).is_none(), "root event has no event span")?;
    ensure(context.event_scope(&root_event).is_none(), "root event has no event scope")?;

    let scope = ensure_some(context.span_scope(child_id), "child span scope exists")?;
    let leaf_to_root = scope.map(|span_ref| span_ref.id()).collect::<Vec<_>>();
    ensure(leaf_to_root == vec![child_id, root_id], "span scope iterates from child to root")?;

    let event_scope = ensure_some(context.event_scope(&explicit_event), "explicit event scope exists")?;
    let root_to_leaf = event_scope.root_to_leaf().map(|span_ref| span_ref.id()).collect::<Vec<_>>();
    ensure(
      root_to_leaf == vec![root_id],
      "explicit parent event scope iterates from root to leaf",
    )?;

    ensure_ok(registry.exit(child_id), "registry exits child span")?;
    ensure(context.current_span().id().is_none(), "context has no current span after exit")
  }

  /// Tests for how max-level hints are calculated when combining layers
  /// with and without per-layer filtering.
  mod max_level_hints {

    use super::*;
    use crate::filter::*;

    fn ensure_max_level_hint(actual: Option<LevelFilter>, expected: Option<LevelFilter>) -> Result<(), TestFailure> {
      ensure(actual == expected, "max level hint matches expected value")
    }

    #[test]
    fn mixed_with_unfiltered() -> Result<(), TestFailure> {
      let subscriber = crate::registry().with(NopLayer).with(NopLayer.with_filter(LevelFilter::INFO));
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn mixed_with_unfiltered_layered() -> Result<(), TestFailure> {
      let subscriber = crate::registry().with(NopLayer).with(
        NopLayer
          .with_filter(LevelFilter::INFO)
          .and_then(NopLayer.with_filter(LevelFilter::TRACE)),
      );
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn mixed_interleaved() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer)
        .with(NopLayer.with_filter(LevelFilter::INFO))
        .with(NopLayer)
        .with(NopLayer.with_filter(LevelFilter::INFO));
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn mixed_layered() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer.with_filter(LevelFilter::INFO).and_then(NopLayer))
        .with(NopLayer.and_then(NopLayer.with_filter(LevelFilter::INFO)));
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn plf_only_unhinted() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer.with_filter(LevelFilter::INFO))
        .with(NopLayer.with_filter(filter_fn(|_| true)));
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn plf_only_unhinted_nested_outer() -> Result<(), TestFailure> {
      // if a nested tree of per-layer filters has an _outer_ filter with
      // no max level hint, it should return `None`.
      let subscriber = crate::registry()
        .with(
          NopLayer
            .with_filter(LevelFilter::INFO)
            .and_then(NopLayer.with_filter(LevelFilter::WARN)),
        )
        .with(
          NopLayer
            .with_filter(filter_fn(|_| true))
            .and_then(NopLayer.with_filter(LevelFilter::DEBUG)),
        );
      ensure_max_level_hint(subscriber.max_level_hint(), None)
    }

    #[test]
    fn plf_only_unhinted_nested_inner() -> Result<(), TestFailure> {
      // If a nested tree of per-layer filters has an _inner_ filter with
      // no max-level hint, but the _outer_ filter has a max level hint,
      // it should pick the outer hint. This is because the outer filter
      // will disable the spans/events before they make it to the inner
      // filter.
      let subscriber = crate::registry().with(
        NopLayer
          .with_filter(filter_fn(|_| true))
          .and_then(NopLayer.with_filter(filter_fn(|_| true)))
          .with_filter(LevelFilter::INFO),
      );
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::INFO))
    }

    #[test]
    fn unhinted_nested_inner() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer.and_then(NopLayer).with_filter(LevelFilter::INFO))
        .with(
          NopLayer
            .with_filter(filter_fn(|_| true))
            .and_then(NopLayer.with_filter(filter_fn(|_| true)))
            .with_filter(LevelFilter::WARN),
        );
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::INFO))
    }

    #[test]
    fn unhinted_nested_inner_mixed() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(
          NopLayer
            .and_then(NopLayer.with_filter(filter_fn(|_| true)))
            .with_filter(LevelFilter::INFO),
        )
        .with(
          NopLayer
            .with_filter(filter_fn(|_| true))
            .and_then(NopLayer.with_filter(filter_fn(|_| true)))
            .with_filter(LevelFilter::WARN),
        );
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::INFO))
    }

    #[test]
    fn plf_only_picks_max() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer.with_filter(LevelFilter::WARN))
        .with(NopLayer.with_filter(LevelFilter::DEBUG));
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::DEBUG))
    }

    #[test]
    fn many_plf_only_picks_max() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(NopLayer.with_filter(LevelFilter::WARN))
        .with(NopLayer.with_filter(LevelFilter::DEBUG))
        .with(NopLayer.with_filter(LevelFilter::INFO))
        .with(NopLayer.with_filter(LevelFilter::ERROR));
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::DEBUG))
    }

    #[test]
    fn nested_plf_only_picks_max() -> Result<(), TestFailure> {
      let subscriber = crate::registry()
        .with(
          NopLayer.with_filter(LevelFilter::INFO).and_then(
            NopLayer
              .with_filter(LevelFilter::WARN)
              .and_then(NopLayer.with_filter(LevelFilter::DEBUG)),
          ),
        )
        .with(
          NopLayer
            .with_filter(LevelFilter::INFO)
            .and_then(NopLayer.with_filter(LevelFilter::ERROR)),
        );
      ensure_max_level_hint(subscriber.max_level_hint(), Some(LevelFilter::DEBUG))
    }
  }
}
