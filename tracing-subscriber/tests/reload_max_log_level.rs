//! Tests reloadable filters updating log max levels.
#![cfg(all(feature = "env-filter", feature = "tracing-log"))]

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure, ensure_ok};
    use tracing::{self, Level};
    use tracing_mock::{expect, subscriber};
    use tracing_subscriber::{filter::LevelFilter, prelude::*, reload};

    #[test]
    fn reload_max_log_level() -> Result<(), TestFailure> {
        let (subscriber, mock_handle) = subscriber::mock()
            .event(expect::event().at_level(Level::INFO))
            .event(expect::event().at_level(Level::DEBUG))
            .event(expect::event().at_level(Level::INFO))
            .only()
            .run_with_handle();
        let (filter, reload_handle) = reload::Layer::new(LevelFilter::INFO);
        ensure_ok(subscriber.with(filter).try_init(), "subscriber installs")?;

        ensure(
            log::log_enabled!(log::Level::Info),
            "info logs start enabled",
        )?;
        ensure(
            !log::log_enabled!(log::Level::Debug),
            "debug logs start disabled",
        )?;
        ensure(
            !log::log_enabled!(log::Level::Trace),
            "trace logs start disabled",
        )?;

        log::debug!("i'm disabled");
        log::info!("i'm enabled");

        ensure_ok(reload_handle.reload(Level::DEBUG), "reloading succeeds")?;

        ensure(
            log::log_enabled!(log::Level::Info),
            "info logs stay enabled",
        )?;
        ensure(
            log::log_enabled!(log::Level::Debug),
            "debug logs become enabled",
        )?;
        ensure(
            !log::log_enabled!(log::Level::Trace),
            "trace logs stay disabled",
        )?;

        log::debug!("i'm enabled now");
        log::info!("i'm still enabled, too");

        ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
