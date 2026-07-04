use core::num::NonZeroU64;

use strict_test_support::TestFailure;
use strict_test_support::ensure;
use strict_test_support::ensure_some;
use tracing::subscriber;
use tracing_core::subscriber::NoSubscriber;
use tracing_core::subscriber::SubscriberResult;

use super::*;

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

#[cfg(all(feature = "registry", feature = "std"))]
mod registry_tests {
  use super::*;
  use crate::registry::LookupSpan;

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
