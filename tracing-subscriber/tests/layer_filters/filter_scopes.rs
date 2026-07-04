use strict_test_support::TestFailure;
use strict_test_support::ensure_ok;
use tracing_mock::expect;
use tracing_mock::layer::MockLayer;

use super::*;

#[test]
fn filters_span_scopes() -> Result<(), TestFailure> {
  let (debug_layer, debug_handle) = layer::named("debug")
    .enter(expect::span().at_level(Level::DEBUG))
    .enter(expect::span().at_level(Level::INFO))
    .enter(expect::span().at_level(Level::WARN))
    .enter(expect::span().at_level(Level::ERROR))
    .event(expect::event().with_fields(expect::msg("hello world")).in_scope(vec![
      expect::span().at_level(Level::ERROR),
      expect::span().at_level(Level::WARN),
      expect::span().at_level(Level::INFO),
      expect::span().at_level(Level::DEBUG),
    ]))
    .exit(expect::span().at_level(Level::ERROR))
    .exit(expect::span().at_level(Level::WARN))
    .exit(expect::span().at_level(Level::INFO))
    .exit(expect::span().at_level(Level::DEBUG))
    .only()
    .run_with_handle();
  let (info_layer, info_handle) = layer::named("info")
    .enter(expect::span().at_level(Level::INFO))
    .enter(expect::span().at_level(Level::WARN))
    .enter(expect::span().at_level(Level::ERROR))
    .event(expect::event().with_fields(expect::msg("hello world")).in_scope(vec![
      expect::span().at_level(Level::ERROR),
      expect::span().at_level(Level::WARN),
      expect::span().at_level(Level::INFO),
    ]))
    .exit(expect::span().at_level(Level::ERROR))
    .exit(expect::span().at_level(Level::WARN))
    .exit(expect::span().at_level(Level::INFO))
    .only()
    .run_with_handle();
  let (warn_layer, warn_handle) = layer::named("warn")
    .enter(expect::span().at_level(Level::WARN))
    .enter(expect::span().at_level(Level::ERROR))
    .event(
      expect::event()
        .with_fields(expect::msg("hello world"))
        .in_scope(vec![expect::span().at_level(Level::ERROR), expect::span().at_level(Level::WARN)]),
    )
    .exit(expect::span().at_level(Level::ERROR))
    .exit(expect::span().at_level(Level::WARN))
    .only()
    .run_with_handle();

  let subscriber = tracing_subscriber::registry()
    .with(debug_layer.with_filter(LevelFilter::DEBUG))
    .with(info_layer.with_filter(LevelFilter::INFO))
    .with(warn_layer.with_filter(LevelFilter::WARN));
  let _subscriber = set_default(subscriber);

  {
    let _trace = tracing::trace_span!("my_span").entered();
    let _debug = tracing::debug_span!("my_span").entered();
    let _info = tracing::info_span!("my_span").entered();
    let _warn = tracing::warn_span!("my_span").entered();
    let _error = tracing::error_span!("my_span").entered();
    tracing::error!("hello world");
  };

  ensure_ok(debug_handle.finished(), "mock expectations should finish")?;
  ensure_ok(info_handle.finished(), "mock expectations should finish")?;
  ensure_ok(warn_handle.finished(), "mock expectations should finish")?;
  Ok(())
}

#[test]
fn filters_interleaved_span_scopes() -> Result<(), TestFailure> {
  fn target_layer(target: &'static str) -> (MockLayer, subscriber::MockHandle) {
    layer::named(format!("target_{target}"))
      .enter(expect::span().with_target(target))
      .enter(expect::span().with_target(target))
      .event(
        expect::event()
          .with_fields(expect::msg("hello world"))
          .in_scope(vec![expect::span().with_target(target), expect::span().with_target(target)]),
      )
      .event(
        expect::event()
          .with_fields(expect::msg("hello to my target"))
          .in_scope(vec![expect::span().with_target(target), expect::span().with_target(target)])
          .with_target(target),
      )
      .exit(expect::span().with_target(target))
      .exit(expect::span().with_target(target))
      .only()
      .run_with_handle()
  }

  let (a_layer, a_handle) = target_layer("a");
  let (b_layer, b_handle) = target_layer("b");
  let (all_layer, all_handle) = layer::named("all")
    .enter(expect::span().with_target("b"))
    .enter(expect::span().with_target("a"))
    .event(
      expect::event()
        .with_fields(expect::msg("hello world"))
        .in_scope(vec![expect::span().with_target("a"), expect::span().with_target("b")]),
    )
    .exit(expect::span().with_target("a"))
    .exit(expect::span().with_target("b"))
    .only()
    .run_with_handle();

  let subscriber = tracing_subscriber::registry()
    .with(all_layer.with_filter(LevelFilter::INFO))
    .with(a_layer.with_filter(filter::filter_fn(|meta| {
      let target = meta.target();
      target == "a" || target == module_path!()
    })))
    .with(b_layer.with_filter(filter::filter_fn(|meta| {
      let target = meta.target();
      target == "b" || target == module_path!()
    })));
  let _subscriber = set_default(subscriber);

  {
    let _a1 = tracing::trace_span!(target: "a", "a/trace").entered();
    let _b1 = tracing::info_span!(target: "b", "b/info").entered();
    let _a2 = tracing::info_span!(target: "a", "a/info").entered();
    let _b2 = tracing::trace_span!(target: "b", "b/trace").entered();
    tracing::info!("hello world");
    tracing::debug!(target: "a", "hello to my target");
    tracing::debug!(target: "b", "hello to my target");
  };

  ensure_ok(a_handle.finished(), "mock expectations should finish")?;
  ensure_ok(b_handle.finished(), "mock expectations should finish")?;
  ensure_ok(all_handle.finished(), "mock expectations should finish")?;
  Ok(())
}
