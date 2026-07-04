//! Tests for using `EnvFilter` as a per-layer filter (rather than a global
//! `Layer` filter).
#![cfg(feature = "registry")]
use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing::subscriber::set_default;
use tracing_mock::expect;
use tracing_mock::layer;

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
