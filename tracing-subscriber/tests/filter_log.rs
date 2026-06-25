//! Tests log compatibility with subscriber filters.
#![cfg(all(feature = "env-filter", feature = "tracing-log"))]

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure, ensure_ok};
    use tracing::{self, Level};
    use tracing_mock::*;
    use tracing_subscriber::{filter::EnvFilter, prelude::*};

    mod my_module {
        use super::{TestFailure, ensure};

        #[allow(
            clippy::single_call_fn,
            reason = "log filter tests keep records in a nested module to exercise module-target filtering"
        )]
        pub(super) fn test_records() {
            log::trace!("this should be disabled");
            log::info!("this shouldn't be");
            log::debug!("this should be disabled");
            log::warn!("this should be enabled");
            log::warn!(target: "something else", "this shouldn't be enabled");
            log::error!("this should be enabled too");
        }

        #[allow(
            clippy::single_call_fn,
            reason = "log filter tests keep enabled checks in a nested module to exercise module-target filtering"
        )]
        pub(super) fn test_log_enabled() -> Result<(), TestFailure> {
            ensure(
                log::log_enabled!(log::Level::Info),
                "info is enabled inside `my_module`",
            )?;
            ensure(
                !log::log_enabled!(log::Level::Debug),
                "debug is disabled inside `my_module`",
            )?;
            ensure(
                log::log_enabled!(log::Level::Warn),
                "warn is enabled inside `my_module`",
            )
        }
    }

    #[test]
    fn log_is_enabled() -> Result<(), TestFailure> {
        let filter: EnvFilter = ensure_ok(
            "filter_log::tests::my_module=info".parse(),
            "log compatibility filter parses",
        )?;
        let (subscriber, mock_handle) = subscriber::mock()
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::WARN))
            .event(expect::event().at_level(Level::ERROR))
            .only()
            .run_with_handle();

        // Note: we have to set the global default in order to set the `log` max
        // level, which can only be set once.
        ensure_ok(subscriber.with(filter).try_init(), "subscriber installs")?;

        my_module::test_records();
        log::info!("this is disabled");

        my_module::test_log_enabled()?;
        ensure(
            !log::log_enabled!(log::Level::Info),
            "info is disabled outside `my_module`",
        )?;
        ensure(
            !log::log_enabled!(log::Level::Warn),
            "warn is disabled outside `my_module`",
        )?;

        ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
