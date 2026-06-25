use super::*;
use strict_test_support::{TestFailure, ensure, ensure_ok};
use tracing::Subscriber;
use tracing::subscriber::set_default;
use tracing_subscriber::{
    Layer as _,
    filter::{self, LevelFilter},
    prelude::*,
};

fn ensure_hint<S>(
    subscriber: &S,
    expected: Option<LevelFilter>,
    context: &'static str,
) -> Result<(), TestFailure>
where
    S: Subscriber,
{
    ensure(subscriber.max_level_hint() == expected, context)
}

#[test]
fn option_some() -> Result<(), TestFailure> {
    let (raw_layer, handle) = layer::mock().only().run_with_handle();
    let filtered_layer = raw_layer.with_filter(Some(filter::dynamic_filter_fn(|_, _| false)));

    let _guard = set_default(tracing_subscriber::registry().with(filtered_layer));

    for input in 0..2 {
        tracing::info!(input);
    }

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn option_none() -> Result<(), TestFailure> {
    let (raw_layer, handle) = layer::mock()
        .event(expect::event())
        .event(expect::event())
        .only()
        .run_with_handle();
    let filtered_layer = raw_layer.with_filter(None::<filter::DynFilterFn<_>>);

    let _guard = set_default(tracing_subscriber::registry().with(filtered_layer));

    for input in 0..2 {
        tracing::info!(input);
    }

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn option_mixed() -> Result<(), TestFailure> {
    let (raw_layer, handle) = layer::mock()
        .event(expect::event())
        .only()
        .run_with_handle();
    let filtered_layer = raw_layer
        .with_filter(filter::dynamic_filter_fn(|meta, _ctx| {
            meta.target() == "interesting"
        }))
        .with_filter(None::<filter::DynFilterFn<_>>);

    let _guard = set_default(tracing_subscriber::registry().with(filtered_layer));

    tracing::info!(target: "interesting", x="foo");
    tracing::info!(target: "boring", x="bar");

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn none_max_level_hint() -> Result<(), TestFailure> {
    let (raw_none_layer, handle_none) = layer::mock()
        .event(expect::event())
        .event(expect::event())
        .only()
        .run_with_handle();
    let filtered_none_layer = raw_none_layer.with_filter(None::<filter::DynFilterFn<_>>);
    let filtered_none_hint = ensure_ok(
        filtered_none_layer.max_level_hint(),
        "None filter max level hint returns",
    )?;
    ensure(
        filtered_none_hint.is_none(),
        "None filter does not provide a max level hint",
    )?;

    let (raw_filter_fn_layer, handle_filter_fn) = layer::mock()
        .event(expect::event())
        .only()
        .run_with_handle();
    let max_level = Level::INFO;
    let filtered_fn_layer = raw_filter_fn_layer.with_filter(
        filter::dynamic_filter_fn(move |meta, _| meta.level() <= &max_level)
            .with_max_level_hint(max_level),
    );
    let filtered_fn_hint = ensure_ok(
        filtered_fn_layer.max_level_hint(),
        "filter function max level hint returns",
    )?;
    ensure(
        filtered_fn_hint == Some(LevelFilter::INFO),
        "filter function provides info max level hint",
    )?;

    let subscriber = tracing_subscriber::registry()
        .with(filtered_none_layer)
        .with(filtered_fn_layer);
    ensure_hint(
        &subscriber,
        None,
        "None filter upgrades the sibling filter hint",
    )?;

    let _guard = set_default(subscriber);
    tracing::info!(target: "interesting", x="foo");
    tracing::debug!(target: "sometimes_interesting", x="bar");

    ensure_ok(handle_none.finished(), "mock expectations should finish")?;
    ensure_ok(
        handle_filter_fn.finished(),
        "mock expectations should finish",
    )?;
    Ok(())
}

#[test]
fn some_max_level_hint() -> Result<(), TestFailure> {
    let (raw_some_layer, handle_some) = layer::mock()
        .event(expect::event())
        .event(expect::event())
        .only()
        .run_with_handle();
    let filtered_some_layer = raw_some_layer.with_filter(Some(
        filter::dynamic_filter_fn(move |meta, _| meta.level() <= &Level::DEBUG)
            .with_max_level_hint(Level::DEBUG),
    ));
    let filtered_some_hint = ensure_ok(
        filtered_some_layer.max_level_hint(),
        "Some filter max level hint returns",
    )?;
    ensure(
        filtered_some_hint == Some(LevelFilter::DEBUG),
        "Some filter propagates debug max level hint",
    )?;

    let (raw_filter_fn_layer, handle_filter_fn) = layer::mock()
        .event(expect::event())
        .only()
        .run_with_handle();
    let filtered_fn_layer = raw_filter_fn_layer.with_filter(
        filter::dynamic_filter_fn(move |meta, _| meta.level() <= &Level::INFO)
            .with_max_level_hint(Level::INFO),
    );
    let filtered_fn_hint = ensure_ok(
        filtered_fn_layer.max_level_hint(),
        "filter function max level hint returns",
    )?;
    ensure(
        filtered_fn_hint == Some(LevelFilter::INFO),
        "filter function provides info max level hint",
    )?;

    let subscriber = tracing_subscriber::registry()
        .with(filtered_some_layer)
        .with(filtered_fn_layer);
    ensure_hint(
        &subscriber,
        Some(LevelFilter::DEBUG),
        "Some filter upgrades sibling filter hint",
    )?;

    let _guard = set_default(subscriber);
    tracing::info!(target: "interesting", x="foo");
    tracing::debug!(target: "sometimes_interesting", x="bar");

    ensure_ok(handle_some.finished(), "mock expectations should finish")?;
    ensure_ok(
        handle_filter_fn.finished(),
        "mock expectations should finish",
    )?;
    Ok(())
}
