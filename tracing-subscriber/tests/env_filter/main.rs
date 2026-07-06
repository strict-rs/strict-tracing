//! Tests environment filter directive behavior.
#![cfg(feature = "env-filter")]

use std::fmt;
use std::sync::Arc;

#[cfg(test)]
mod per_layer;

use parking_lot::Mutex;
use strict_test_support::TestFailure;
use strict_test_support::ensure;
use strict_test_support::ensure_eq;
use strict_test_support::ensure_ok;
use tracing::Level;
use tracing::field::Field;
use tracing::field::Visit;
use tracing::subscriber::with_default;
use tracing_core::Subscriber;
use tracing_core::span;
use tracing_core::subscriber::SubscriberResult;
use tracing_mock::expect;
use tracing_mock::layer;
use tracing_mock::subscriber;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;

#[derive(Clone, Default)]
struct SpanRecorder {
  spans: Arc<Mutex<Vec<RecordedSpan>>>,
}

struct RecordedSpan {
  level: Level,
  hello: Option<u64>,
}

#[derive(Default)]
struct RecordedFields {
  hello: Option<u64>,
}

impl Visit for RecordedFields {
  fn record_u64(&mut self, field: &Field, field_value: u64) {
    if field.name() == "hello" {
      self.hello = Some(field_value);
    }
  }

  fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}
}

impl<S> tracing_subscriber::Layer<S> for SpanRecorder
where
  S: Subscriber,
{
  fn on_new_span(&self, attrs: &span::Attributes<'_>, _id: span::Id, _ctx: Context<'_, S>) -> SubscriberResult {
    let mut fields = RecordedFields::default();
    attrs.record(&mut fields);

    self.spans.lock().push(RecordedSpan {
      level: *attrs.metadata().level(),
      hello: fields.hello,
    });
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn level_filter_event() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("info".parse(), "level event filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "foo", "this should also be disabled");
      tracing::warn!(target: "foo", "this should be enabled");
      tracing::error!("this should be enabled too");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn same_name_spans() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("[foo{bar}]=trace,[foo{baz}]=trace".parse(), "same-name span filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .new_span(
        expect::span()
          .named("foo")
          .at_level(Level::TRACE)
          .with_fields(expect::field("bar")),
      )
      .new_span(
        expect::span()
          .named("foo")
          .at_level(Level::TRACE)
          .with_fields(expect::field("baz")),
      )
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);
    with_default(subscriber, || {
      tracing::trace_span!("foo", bar = 1);
      tracing::trace_span!("foo", baz = 1);
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn level_filter_event_with_target() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("info,stuff=debug".parse(), "targeted level event filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::DEBUG).with_target("stuff"))
      .event(expect::event().at_level(Level::WARN).with_target("stuff"))
      .event(expect::event().at_level(Level::ERROR))
      .event(expect::event().at_level(Level::ERROR).with_target("stuff"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "stuff", "this should be enabled");
      tracing::debug!("but this shouldn't");
      tracing::trace!(target: "stuff", "and neither should this");
      tracing::warn!(target: "stuff", "this should be enabled");
      tracing::error!("this should be enabled too");
      tracing::error!(target: "stuff", "this should be enabled also");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn level_filter_event_with_target_and_span_global() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("info,stuff[cool_span]=debug".parse(), "target-and-span global filter parses")?;

    let cool_span = expect::span().named("cool_span");
    let (layer, handle) = layer::mock()
      .enter(&cool_span)
      .event(expect::event().at_level(Level::DEBUG).in_scope(vec![cool_span.clone()]))
      .exit(cool_span)
      .enter("uncool_span")
      .exit("uncool_span")
      .only()
      .run_with_handle();

    let subscriber = Registry::default().with(filter).with(layer);

    with_default(subscriber, || {
      {
        let _span = tracing::info_span!(target: "stuff", "cool_span").entered();
        tracing::debug!("this should be enabled");
      };

      tracing::debug!("should also be disabled");

      {
        let _span = tracing::info_span!("uncool_span").entered();
        tracing::debug!("this should be disabled");
      };
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn not_order_dependent() -> Result<(), TestFailure> {
    // this test reproduces tokio-rs/tracing#623

    let filter: EnvFilter = ensure_ok("stuff=debug,info".parse(), "order-independent filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::DEBUG).with_target("stuff"))
      .event(expect::event().at_level(Level::WARN).with_target("stuff"))
      .event(expect::event().at_level(Level::ERROR))
      .event(expect::event().at_level(Level::ERROR).with_target("stuff"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "stuff", "this should be enabled");
      tracing::debug!("but this shouldn't");
      tracing::trace!(target: "stuff", "and neither should this");
      tracing::warn!(target: "stuff", "this should be enabled");
      tracing::error!("this should be enabled too");
      tracing::error!(target: "stuff", "this should be enabled also");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn add_directive_enables_event() -> Result<(), TestFailure> {
    // this test reproduces tokio-rs/tracing#591

    // by default, use info level
    let mut filter = EnvFilter::new(LevelFilter::INFO.to_string());

    // overwrite with a more specific directive
    filter = filter.add_directive(ensure_ok("hello=trace".parse(), "hello trace directive parses")?);

    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO).with_target("hello"))
      .event(expect::event().at_level(Level::TRACE).with_target("hello"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::info!(target: "hello", "hello info");
      tracing::trace!(target: "hello", "hello trace");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn span_name_filter_is_dynamic() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("info,[cool_span]=debug".parse(), "span-name dynamic filter parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .enter(expect::span().named("cool_span"))
      .event(expect::event().at_level(Level::DEBUG))
      .enter(expect::span().named("uncool_span"))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::DEBUG))
      .exit(expect::span().named("uncool_span"))
      .exit(expect::span().named("cool_span"))
      .enter(expect::span().named("uncool_span"))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .exit(expect::span().named("uncool_span"))
      .only()
      .run_with_handle();
    let subscriber = mock_subscriber.with(filter);

    with_default(subscriber, || {
      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      let cool_span = tracing::info_span!("cool_span");
      let uncool_span = tracing::info_span!("uncool_span");

      {
        let _enter = cool_span.enter();
        tracing::debug!("i'm a cool event");
        tracing::trace!("i'm cool, but not cool enough");
        let _enter2 = uncool_span.enter();
        tracing::warn!("warning: extremely cool!");
        tracing::debug!("i'm still cool");
      };

      let _enter = uncool_span.enter();
      tracing::warn!("warning: not that cool");
      tracing::trace!("im not cool enough");
      tracing::error!("uncool error");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn method_name_resolution() {
    let filter = EnvFilter::new("hello_world=info");
    let _hint = <EnvFilter as tracing_subscriber::Layer<Registry>>::max_level_hint(&filter);
  }

  #[test]
  fn parse_invalid_string() -> Result<(), TestFailure> {
    ensure(EnvFilter::builder().parse(",!").is_err(), "invalid filter string fails to parse")
  }

  #[test]
  fn parse_empty_string_no_default_directive() -> Result<(), TestFailure> {
    let filter = ensure_ok(EnvFilter::builder().parse(""), "empty filter without default directive parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock().only().run_with_handle();
    let layer = mock_subscriber.with(filter);

    with_default(layer, || {
      tracing::trace!("this should be disabled");
      tracing::debug!("this should be disabled");
      tracing::info!("this should be disabled");
      tracing::warn!("this should be disabled");
      tracing::error!("this should be disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn parse_empty_string_with_default_directive() -> Result<(), TestFailure> {
    let filter = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).parse("");
    let parsed_filter = ensure_ok(filter, "empty filter with default directive parses")?;
    let (mock_subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let layer = mock_subscriber.with(parsed_filter);

    with_default(layer, || {
      tracing::trace!("this should be disabled");
      tracing::debug!("this should be disabled");
      tracing::info!("this shouldn't be disabled");
      tracing::warn!("this shouldn't be disabled");
      tracing::error!("this shouldn't be disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn new_invalid_string() -> Result<(), TestFailure> {
    let filter = EnvFilter::new(",!");
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let layer = subscriber.with(filter);

    with_default(layer, || {
      tracing::trace!("this should be disabled");
      tracing::debug!("this should be disabled");
      tracing::info!("this should be disabled");
      tracing::warn!("this should be disabled");
      tracing::error!("this shouldn't be disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn new_empty_string() -> Result<(), TestFailure> {
    let filter = EnvFilter::new("");
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let layer = subscriber.with(filter);

    with_default(layer, || {
      tracing::trace!("this should be disabled");
      tracing::debug!("this should be disabled");
      tracing::info!("this should be disabled");
      tracing::warn!("this should be disabled");
      tracing::error!("this shouldn't be disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn more_specific_static_filter_more_verbose() -> Result<(), TestFailure> {
    let filter = EnvFilter::new("info,hello=debug");
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::DEBUG).with_target("hello"))
      .only()
      .run_with_handle();
    let layer = subscriber.with(filter);

    with_default(layer, || {
      tracing::info!("should be enabled");
      tracing::debug!("should be disabled");
      tracing::debug!(target: "hello", "should be enabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn more_specific_static_filter_less_verbose() -> Result<(), TestFailure> {
    let filter = EnvFilter::new("info,hello=warn");
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN).with_target("env_filter::tests"))
      .only()
      .run_with_handle();
    let layer = subscriber.with(filter);

    with_default(layer, || {
      tracing::info!("should be enabled");
      tracing::warn!("should be enabled");
      tracing::info!(target: "hello", "should be disabled");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  fn more_specific_dynamic_filter_more_verbose() -> Result<(), TestFailure> {
    let filter = EnvFilter::new("info,[{hello=4}]=debug");
    let (subscriber, mock_handle) = subscriber::mock()
      .new_span(expect::span().at_level(Level::INFO))
      .close_span("enabled info")
      .new_span(
        expect::span()
          .at_level(Level::DEBUG)
          .with_fields(expect::field("hello").with_value(&4_u64)),
      )
      .close_span("enabled debug")
      .event(expect::event().with_fields(expect::msg("marker")))
      .only()
      .run_with_handle();
    let layer = subscriber.with(filter);

    with_default(layer, || {
      tracing::info_span!("enabled info");
      tracing::debug_span!("disabled debug");
      tracing::debug_span!("enabled debug", hello = &4_u64);

      // .only() doesn't work when we don't enter/exit spans
      tracing::info!("marker");
    });

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  /// This pins the current issue-1388 behavior without relying on a panic.
  ///
  /// Fixing this test would resolve <https://github.com/tokio-rs/tracing/issues/1388>
  /// (and probably a few more issues as well). When that happens, this should
  /// fail with a structured [`TestFailure`] and the expected span sequence can
  /// be updated to the fixed behavior.
  #[test]
  fn more_specific_dynamic_filter_less_verbose() -> Result<(), TestFailure> {
    let filter = EnvFilter::new("info,[{hello=4}]=warn");
    let spans = Arc::new(Mutex::new(Vec::new()));
    let recorder = SpanRecorder {
      spans: Arc::clone(&spans)
    };
    let subscriber = Registry::default().with(filter).with(recorder);

    with_default(subscriber, || {
      tracing::info_span!("enabled info");
      tracing::warn_span!("enabled hello=100 warn", hello = &100_u64);
      tracing::info_span!("disabled hello=4 info", hello = &4_u64);
      tracing::warn_span!("enabled hello=4 warn", hello = &4_u64);

      tracing::info!("marker");
    });

    let observed = spans
      .lock()
      .iter()
      .map(|span| {
        let hello = span.hello.map_or_else(|| "-".to_owned(), |value| value.to_string());
        format!("{}:{hello}", span.level.as_str())
      })
      .collect::<Vec<_>>()
      .join(",");
    let expected = "INFO:-,WARN:100,INFO:4,WARN:4".to_owned();

    ensure_eq(
      &observed,
      &expected,
      "issue-1388 less-verbose dynamic filter span sequence is pinned",
    )
  }

  // contains the same tests as the first half of this file
  // but using EnvFilter as a `Filter`, not as a `Layer`
  mod per_layer_filter {
    use tracing::subscriber::set_default;

    use super::*;

    #[test]
    fn level_filter_event() -> Result<(), TestFailure> {
      let filter: EnvFilter = ensure_ok("info".parse(), "per-layer level filter parses")?;
      let (layer, handle) = layer::mock()
        .event(expect::event().at_level(Level::INFO))
        .event(expect::event().at_level(Level::WARN))
        .event(expect::event().at_level(Level::ERROR))
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "foo", "this should also be disabled");
      tracing::warn!(target: "foo", "this should be enabled");
      tracing::error!("this should be enabled too");

      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn same_name_spans() -> Result<(), TestFailure> {
      let filter: EnvFilter = ensure_ok(
        "[foo{bar}]=trace,[foo{baz}]=trace".parse(),
        "per-layer same-name span filter parses",
      )?;
      let (layer, handle) = layer::mock()
        .new_span(
          expect::span()
            .named("foo")
            .at_level(Level::TRACE)
            .with_fields(expect::field("bar")),
        )
        .new_span(
          expect::span()
            .named("foo")
            .at_level(Level::TRACE)
            .with_fields(expect::field("baz")),
        )
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::trace_span!("foo", bar = 1);
      tracing::trace_span!("foo", baz = 1);

      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn level_filter_event_with_target() -> Result<(), TestFailure> {
      let filter: EnvFilter = ensure_ok("info,stuff=debug".parse(), "per-layer targeted level filter parses")?;
      let (layer, handle) = layer::mock()
        .event(expect::event().at_level(Level::INFO))
        .event(expect::event().at_level(Level::DEBUG).with_target("stuff"))
        .event(expect::event().at_level(Level::WARN).with_target("stuff"))
        .event(expect::event().at_level(Level::ERROR))
        .event(expect::event().at_level(Level::ERROR).with_target("stuff"))
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "stuff", "this should be enabled");
      tracing::debug!("but this shouldn't");
      tracing::trace!(target: "stuff", "and neither should this");
      tracing::warn!(target: "stuff", "this should be enabled");
      tracing::error!("this should be enabled too");
      tracing::error!(target: "stuff", "this should be enabled also");

      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn level_filter_event_with_target_and_span() -> Result<(), TestFailure> {
      let filter: EnvFilter = ensure_ok("stuff[cool_span]=debug".parse(), "per-layer target-and-span filter parses")?;

      let cool_span = expect::span().named("cool_span");
      let (layer, handle) = layer::mock()
        .enter(cool_span.clone())
        .event(expect::event().at_level(Level::DEBUG).in_scope(vec![cool_span.clone()]))
        .exit(cool_span)
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      {
        let _span = tracing::info_span!(target: "stuff", "cool_span").entered();
        tracing::debug!("this should be enabled");
      };

      tracing::debug!("should also be disabled");

      {
        let _span = tracing::info_span!("uncool_span").entered();
        tracing::debug!("this should be disabled");
      };

      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn not_order_dependent() -> Result<(), TestFailure> {
      // this test reproduces tokio-rs/tracing#623

      let filter: EnvFilter = ensure_ok("stuff=debug,info".parse(), "per-layer order-independent filter parses")?;
      let (layer, mock_handle) = layer::mock()
        .event(expect::event().at_level(Level::INFO))
        .event(expect::event().at_level(Level::DEBUG).with_target("stuff"))
        .event(expect::event().at_level(Level::WARN).with_target("stuff"))
        .event(expect::event().at_level(Level::ERROR))
        .event(expect::event().at_level(Level::ERROR).with_target("stuff"))
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      tracing::debug!(target: "stuff", "this should be enabled");
      tracing::debug!("but this shouldn't");
      tracing::trace!(target: "stuff", "and neither should this");
      tracing::warn!(target: "stuff", "this should be enabled");
      tracing::error!("this should be enabled too");
      tracing::error!(target: "stuff", "this should be enabled also");

      ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn add_directive_enables_event() -> Result<(), TestFailure> {
      // this test reproduces tokio-rs/tracing#591

      // by default, use info level
      let mut filter = EnvFilter::new(LevelFilter::INFO.to_string());

      // overwrite with a more specific directive
      filter = filter.add_directive(ensure_ok("hello=trace".parse(), "per-layer hello trace directive parses")?);

      let (layer, mock_handle) = layer::mock()
        .event(expect::event().at_level(Level::INFO).with_target("hello"))
        .event(expect::event().at_level(Level::TRACE).with_target("hello"))
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::info!(target: "hello", "hello info");
      tracing::trace!(target: "hello", "hello trace");

      ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn span_name_filter_is_dynamic() -> Result<(), TestFailure> {
      let filter: EnvFilter = ensure_ok("info,[cool_span]=debug".parse(), "per-layer span-name dynamic filter parses")?;
      let expected_cool_span = expect::span().named("cool_span");
      let expected_uncool_span = expect::span().named("uncool_span");
      let (layer, mock_handle) = layer::mock()
        .event(expect::event().at_level(Level::INFO))
        .enter(expected_cool_span.clone())
        .event(
          expect::event()
            .at_level(Level::DEBUG)
            .in_scope(vec![expected_cool_span.clone()]),
        )
        .enter(expected_uncool_span.clone())
        .event(
          expect::event()
            .at_level(Level::WARN)
            .in_scope(vec![expected_uncool_span.clone()]),
        )
        .event(
          expect::event()
            .at_level(Level::DEBUG)
            .in_scope(vec![expected_uncool_span.clone()]),
        )
        .exit(expected_uncool_span.clone())
        .exit(expected_cool_span)
        .enter(expected_uncool_span.clone())
        .event(
          expect::event()
            .at_level(Level::WARN)
            .in_scope(vec![expected_uncool_span.clone()]),
        )
        .event(
          expect::event()
            .at_level(Level::ERROR)
            .in_scope(vec![expected_uncool_span.clone()]),
        )
        .exit(expected_uncool_span)
        .only()
        .run_with_handle();

      let subscriber = tracing_subscriber::registry().with(layer.with_filter(filter));
      let _subscriber = set_default(subscriber);

      tracing::trace!("this should be disabled");
      tracing::info!("this shouldn't be");
      let cool_span = tracing::info_span!("cool_span");
      let uncool_span = tracing::info_span!("uncool_span");

      {
        let _enter = cool_span.enter();
        tracing::debug!("i'm a cool event");
        tracing::trace!("i'm cool, but not cool enough");
        let _enter2 = uncool_span.enter();
        tracing::warn!("warning: extremely cool!");
        tracing::debug!("i'm still cool");
      };

      {
        let _enter = uncool_span.enter();
        tracing::warn!("warning: not that cool");
        tracing::trace!("im not cool enough");
        tracing::error!("uncool error");
      };

      ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn multiple_dynamic_filters() -> Result<(), TestFailure> {
      // Test that multiple dynamic (span) filters only apply to the layers
      // they're attached to.
      let (layer1, handle1) = {
        let span = expect::span().named("span1");
        let filter: EnvFilter = ensure_ok("[span1]=debug".parse(), "first dynamic per-layer filter parses")?;
        let (layer, handle) = layer::named("layer1")
          .enter(span.clone())
          .event(expect::event().at_level(Level::DEBUG).in_scope(vec![span.clone()]))
          .exit(span)
          .only()
          .run_with_handle();
        (layer.with_filter(filter), handle)
      };

      let (layer2, handle2) = {
        let span = expect::span().named("span2");
        let filter: EnvFilter = ensure_ok("[span2]=info".parse(), "second dynamic per-layer filter parses")?;
        let (layer, handle) = layer::named("layer2")
          .enter(span.clone())
          .event(expect::event().at_level(Level::INFO).in_scope(vec![span.clone()]))
          .exit(span)
          .only()
          .run_with_handle();
        (layer.with_filter(filter), handle)
      };

      let subscriber = tracing_subscriber::registry().with(layer1).with(layer2);
      let _subscriber = set_default(subscriber);

      tracing::info_span!("span1").in_scope(|| {
        tracing::debug!("hello from span 1");
        tracing::trace!("not enabled");
      });

      tracing::info_span!("span2").in_scope(|| {
        tracing::info!("hello from span 2");
        tracing::debug!("not enabled");
      });

      ensure_ok(handle1.finished(), "mock expectations should finish")?;
      ensure_ok(handle2.finished(), "mock expectations should finish")?;
      Ok(())
    }
  }
}
