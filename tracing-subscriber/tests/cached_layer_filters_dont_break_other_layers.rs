//! Tests cached layer filters alongside other layers.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure_ok};
    use tracing::Level;
    use tracing::subscriber::set_default;
    use tracing_mock::{
        expect,
        layer::{self, MockLayer},
        subscriber,
    };
    use tracing_subscriber::{filter::LevelFilter, prelude::*};

    #[test]
    fn layer_filters() -> Result<(), TestFailure> {
        let (unfiltered, unfiltered_handle) = unfiltered("unfiltered");
        let (filtered, filtered_handle) = filtered("filtered");

        let subscriber = tracing_subscriber::registry()
            .with(unfiltered)
            .with(filtered.with_filter(filter()));
        let _subscriber = set_default(subscriber);

        events();

        ensure_ok(
            unfiltered_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered_handle.finished(),
            "mock expectations should finish",
        )?;
        Ok(())
    }

    #[test]
    fn layered_layer_filters() -> Result<(), TestFailure> {
        let (unfiltered1, unfiltered1_handle) = unfiltered("unfiltered_1");
        let (unfiltered2, unfiltered2_handle) = unfiltered("unfiltered_2");
        let unfiltered = unfiltered1.and_then(unfiltered2);

        let (filtered1, filtered1_handle) = filtered("filtered_1");
        let (filtered2, filtered2_handle) = filtered("filtered_2");
        let filtered = filtered1
            .with_filter(filter())
            .and_then(filtered2.with_filter(filter()));

        let subscriber = tracing_subscriber::registry()
            .with(unfiltered)
            .with(filtered);
        let _subscriber = set_default(subscriber);

        events();

        ensure_ok(
            unfiltered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            unfiltered2_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered2_handle.finished(),
            "mock expectations should finish",
        )?;
        Ok(())
    }

    #[test]
    fn out_of_order() -> Result<(), TestFailure> {
        let (unfiltered1, unfiltered1_handle) = unfiltered("unfiltered_1");
        let (unfiltered2, unfiltered2_handle) = unfiltered("unfiltered_2");

        let (filtered1, filtered1_handle) = filtered("filtered_1");
        let (filtered2, filtered2_handle) = filtered("filtered_2");

        let subscriber = tracing_subscriber::registry()
            .with(unfiltered1)
            .with(filtered1.with_filter(filter()))
            .with(unfiltered2)
            .with(filtered2.with_filter(filter()));
        let _subscriber = set_default(subscriber);
        events();

        ensure_ok(
            unfiltered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            unfiltered2_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered2_handle.finished(),
            "mock expectations should finish",
        )?;
        Ok(())
    }

    #[test]
    fn mixed_layered() -> Result<(), TestFailure> {
        let (unfiltered1, unfiltered1_handle) = unfiltered("unfiltered_1");
        let (unfiltered2, unfiltered2_handle) = unfiltered("unfiltered_2");
        let (filtered1, filtered1_handle) = filtered("filtered_1");
        let (filtered2, filtered2_handle) = filtered("filtered_2");

        let layered1 = filtered1.with_filter(filter()).and_then(unfiltered1);
        let layered2 = unfiltered2.and_then(filtered2.with_filter(filter()));

        let subscriber = tracing_subscriber::registry().with(layered1).with(layered2);
        let _subscriber = set_default(subscriber);

        events();

        ensure_ok(
            unfiltered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            unfiltered2_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered1_handle.finished(),
            "mock expectations should finish",
        )?;
        ensure_ok(
            filtered2_handle.finished(),
            "mock expectations should finish",
        )?;
        Ok(())
    }

    fn events() {
        tracing::trace!("hello trace");
        tracing::debug!("hello debug");
        tracing::info!("hello info");
        tracing::warn!("hello warn");
        tracing::error!("hello error");
    }

    const fn filter() -> LevelFilter {
        LevelFilter::INFO
    }

    fn unfiltered(name: &str) -> (MockLayer, subscriber::MockHandle) {
        layer::named(name)
            .event(expect::event().at_level(Level::TRACE))
            .event(expect::event().at_level(Level::DEBUG))
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle()
    }

    fn filtered(name: &str) -> (MockLayer, subscriber::MockHandle) {
        layer::named(name)
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle()
    }
}
