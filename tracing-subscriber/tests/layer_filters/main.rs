//! Tests per-layer filtering behavior.
#![cfg(feature = "registry")]
#[cfg(test)]
mod boxed;
#[cfg(test)]
mod downcast_ref_by_id;
#[cfg(test)]
mod filter_scopes;
#[cfg(test)]
mod option;
#[cfg(test)]
mod per_event;
#[cfg(test)]
mod targets;
#[cfg(test)]
mod trees;
#[cfg(test)]
mod vec;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use strict_test_support::{TestFailure, ensure, ensure_ok};
use tracing::subscriber::set_default;
use tracing::{Level, level_filters::LevelFilter};
use tracing_mock::{expect, layer, subscriber};
use tracing_subscriber::{Layer as _, filter, prelude::*};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_layer_filters() -> Result<(), TestFailure> {
        let (trace_layer, trace_handle) = layer::named("trace")
            .event(expect::event().at_level(Level::TRACE))
            .event(expect::event().at_level(Level::DEBUG))
            .event(expect::event().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let (debug_layer, debug_handle) = layer::named("debug")
            .event(expect::event().at_level(Level::DEBUG))
            .event(expect::event().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let (info_layer, info_handle) = layer::named("info")
            .event(expect::event().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(trace_layer.with_filter(LevelFilter::TRACE))
            .with(debug_layer.with_filter(LevelFilter::DEBUG))
            .with(info_layer.with_filter(LevelFilter::INFO));
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
    fn basic_layer_filter_spans() -> Result<(), TestFailure> {
        let (trace_layer, trace_handle) = layer::named("trace")
            .new_span(expect::span().at_level(Level::TRACE))
            .new_span(expect::span().at_level(Level::DEBUG))
            .new_span(expect::span().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let (debug_layer, debug_handle) = layer::named("debug")
            .new_span(expect::span().at_level(Level::DEBUG))
            .new_span(expect::span().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let (info_layer, info_handle) = layer::named("info")
            .new_span(expect::span().at_level(Level::INFO))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(trace_layer.with_filter(LevelFilter::TRACE))
            .with(debug_layer.with_filter(LevelFilter::DEBUG))
            .with(info_layer.with_filter(LevelFilter::INFO));
        let _subscriber = set_default(subscriber);

        tracing::trace_span!("hello trace");
        tracing::debug_span!("hello debug");
        tracing::info_span!("hello info");

        ensure_ok(trace_handle.finished(), "mock expectations should finish")?;
        ensure_ok(debug_handle.finished(), "mock expectations should finish")?;
        ensure_ok(info_handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn global_filters_subscribers_still_work() -> Result<(), TestFailure> {
        let (expect, handle) = layer::mock()
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(expect)
            .with(LevelFilter::INFO);
        let _subscriber = set_default(subscriber);

        tracing::trace!("hello trace");
        tracing::debug!("hello debug");
        tracing::info!("hello info");
        tracing::warn!("hello warn");
        tracing::error!("hello error");

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn global_filter_interests_are_cached() -> Result<(), TestFailure> {
        let saw_global_filter_violation = Arc::new(AtomicBool::new(false));
        let violation = Arc::clone(&saw_global_filter_violation);
        let (expect, handle) = layer::mock()
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(expect.with_filter(filter::filter_fn(move |meta| {
                if meta.level() > &Level::INFO {
                    violation.store(true, Ordering::SeqCst);
                }
                meta.level() <= &Level::WARN
            })))
            .with(LevelFilter::INFO);
        let _subscriber = set_default(subscriber);

        tracing::trace!("hello trace");
        tracing::debug!("hello debug");
        tracing::info!("hello info");
        tracing::warn!("hello warn");
        tracing::error!("hello error");

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        ensure(
            !saw_global_filter_violation.load(Ordering::SeqCst),
            "enabled is not called for callsites disabled by the global filter",
        )
    }

    #[test]
    fn global_filters_affect_subscriber_filters() -> Result<(), TestFailure> {
        let (expect, handle) = layer::named("debug")
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(expect.with_filter(LevelFilter::DEBUG))
            .with(LevelFilter::INFO);
        let _subscriber = set_default(subscriber);

        tracing::trace!("hello trace");
        tracing::debug!("hello debug");
        tracing::info!("hello info");
        tracing::warn!("hello warn");
        tracing::error!("hello error");

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn filter_fn() -> Result<(), TestFailure> {
        let (all, all_handle) = layer::named("all_targets")
            .event(expect::event().with_fields(expect::msg("hello foo")))
            .event(expect::event().with_fields(expect::msg("hello bar")))
            .only()
            .run_with_handle();

        let (foo, foo_handle) = layer::named("foo_target")
            .event(expect::event().with_fields(expect::msg("hello foo")))
            .only()
            .run_with_handle();

        let (bar, bar_handle) = layer::named("bar_target")
            .event(expect::event().with_fields(expect::msg("hello bar")))
            .only()
            .run_with_handle();

        let subscriber = tracing_subscriber::registry()
            .with(all)
            .with(foo.with_filter(filter::filter_fn(|meta| meta.target().starts_with("foo"))))
            .with(bar.with_filter(filter::filter_fn(|meta| meta.target().starts_with("bar"))));
        let _subscriber = set_default(subscriber);

        tracing::trace!(target: "foo", "hello foo");
        tracing::trace!(target: "bar", "hello bar");

        ensure_ok(foo_handle.finished(), "mock expectations should finish")?;
        ensure_ok(bar_handle.finished(), "mock expectations should finish")?;
        ensure_ok(all_handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
