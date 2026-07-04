use strict_test_support::TestFailure;
use strict_test_support::ensure;
use strict_test_support::ensure_ok;
use tracing::Subscriber;
use tracing::subscriber::set_default;
use tracing_mock::expect;
use tracing_mock::layer::MockLayer;

use super::*;

fn ensure_hint<S>(subscriber: &S, expected: Option<LevelFilter>, context: &'static str) -> Result<(), TestFailure>
where
  S: Subscriber,
{
  ensure(subscriber.max_level_hint() == expected, context)
}

#[test]
fn with_filters_unboxed() -> Result<(), TestFailure> {
  let (raw_trace_layer, trace_handle) = layer::named("trace")
    .event(expect::event().at_level(Level::TRACE))
    .event(expect::event().at_level(Level::DEBUG))
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let trace_layer = raw_trace_layer.with_filter(LevelFilter::TRACE);

  let (raw_debug_layer, debug_handle) = layer::named("debug")
    .event(expect::event().at_level(Level::DEBUG))
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let debug_layer = raw_debug_layer.with_filter(LevelFilter::DEBUG);

  let (raw_info_layer, info_handle) = layer::named("info")
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let info_layer = raw_info_layer.with_filter(LevelFilter::INFO);

  let subscriber = tracing_subscriber::registry().with(vec![trace_layer, debug_layer, info_layer]);
  let _subscriber = set_default(subscriber);

  tracing::trace!("hello trace");
  tracing::debug!("hello debug");
  tracing::info!("hello info");

  ensure_ok(trace_handle.finished(), "mock expectations should finish")?;
  ensure_ok(debug_handle.finished(), "mock expectations should finish")?;
  ensure_ok(info_handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn with_filters_boxed() -> Result<(), TestFailure> {
  let (raw_unfiltered_layer, unfiltered_handle) = layer::named("unfiltered")
    .event(expect::event().at_level(Level::TRACE))
    .event(expect::event().at_level(Level::DEBUG))
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let unfiltered_layer = raw_unfiltered_layer.boxed();

  let (raw_debug_layer, debug_handle) = layer::named("debug")
    .event(expect::event().at_level(Level::DEBUG))
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let debug_layer = raw_debug_layer.with_filter(LevelFilter::DEBUG).boxed();

  let (raw_target_layer, target_handle) = layer::named("target")
    .event(expect::event().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let target_layer = raw_target_layer
    .with_filter(filter::filter_fn(|meta| meta.target() == "my_target"))
    .boxed();

  let subscriber = tracing_subscriber::registry().with(vec![unfiltered_layer, debug_layer, target_layer]);
  let _subscriber = set_default(subscriber);

  tracing::trace!("hello trace");
  tracing::debug!("hello debug");
  tracing::info!(target: "my_target", "hello my target");

  ensure_ok(unfiltered_handle.finished(), "mock expectations should finish")?;
  ensure_ok(debug_handle.finished(), "mock expectations should finish")?;
  ensure_ok(target_handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn mixed_max_level_hint() -> Result<(), TestFailure> {
  let unfiltered = layer::named("unfiltered").run().boxed();
  let info = layer::named("info").run().with_filter(LevelFilter::INFO).boxed();
  let debug = layer::named("debug").run().with_filter(LevelFilter::DEBUG).boxed();

  let subscriber = tracing_subscriber::registry().with(vec![unfiltered, info, debug]);

  ensure_hint(&subscriber, None, "mixed vector layers have no combined max level hint")
}

#[test]
fn all_filtered_max_level_hint() -> Result<(), TestFailure> {
  let warn = layer::named("warn").run().with_filter(LevelFilter::WARN).boxed();
  let info = layer::named("info").run().with_filter(LevelFilter::INFO).boxed();
  let debug = layer::named("debug").run().with_filter(LevelFilter::DEBUG).boxed();

  let subscriber = tracing_subscriber::registry().with(vec![warn, info, debug]);

  ensure_hint(
    &subscriber,
    Some(LevelFilter::DEBUG),
    "all filtered vector layers report the most verbose max level",
  )
}

#[test]
fn empty_vec() -> Result<(), TestFailure> {
  // Just a None means everything is off
  let subscriber = tracing_subscriber::registry().with(Vec::<MockLayer>::new());
  ensure_hint(&subscriber, Some(LevelFilter::OFF), "empty vector disables all levels")
}
