//! Shared log-capture helpers for the `tracing` log integration tests.

use log::{LevelFilter, Log, Metadata, Record};
use parking_lot::Mutex;
use std::sync::Arc;
use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok};

/// Captures the most recent log record emitted by a test.
#[derive(Debug)]
pub struct Test {
    /// Shared captured-log state installed into the global logger.
    state: Arc<State>,
}

/// Mutable log-capture state shared between the test handle and logger.
#[derive(Debug)]
struct State {
    /// Last formatted log message emitted since the previous assertion.
    last_log: Mutex<Option<String>>,
}

/// Logger installed into the `log` facade for one integration-test process.
#[derive(Debug)]
struct Logger {
    /// Per-target maximum levels accepted by this logger.
    filters: Vec<(&'static str, LevelFilter)>,
    /// Shared captured-log state updated when a record is accepted.
    state: Arc<State>,
}

impl Log for Logger {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        for &(target, level) in &self.filters {
            if meta.target().starts_with(target) {
                return meta.level() <= level;
            }
        }
        false
    }

    fn log(&self, record: &Record<'_>) {
        let line = record.args().to_string();
        *self.state.last_log.lock() = Some(line);
    }

    fn flush(&self) {}
}

impl Test {
    /// Installs a logger that captures every log record.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure`] if another logger has already been installed in
    /// the current test process.
    pub fn try_start() -> Result<Self, TestFailure> {
        Self::install_with_filters(&[("", LevelFilter::Trace)])
    }

    /// Installs a logger with per-target maximum log levels.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure`] if another logger has already been installed in
    /// the current test process.
    pub fn try_with_filters(filters: &[(&'static str, LevelFilter)]) -> Result<Self, TestFailure> {
        Self::install_with_filters(filters)
    }

    /// Installs the boxed `log` facade logger for the provided filters.
    fn install_with_filters(filters: &[(&'static str, LevelFilter)]) -> Result<Self, TestFailure> {
        let state = Arc::new(State {
            last_log: Mutex::new(None),
        });
        let logger_state = Arc::clone(&state);
        let max_level = filters
            .iter()
            .fold(LevelFilter::Off, |current_level, filter| {
                current_level.max(filter.1)
            });
        let logger = Logger {
            filters: filters.to_vec(),
            state: logger_state,
        };

        ensure_ok(
            log::set_boxed_logger(Box::new(logger)),
            "test logger installs",
        )?;
        log::set_max_level(max_level);

        Ok(Self { state })
    }

    /// Checks that the most recent captured log line matches `expected`.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure`] if no log line was captured or if the captured
    /// line differs from `expected`.
    pub fn try_assert_logged(&self, expected: &str) -> Result<(), TestFailure> {
        let Some(last_log) = self.take_last_log() else {
            return ensure(false, "a log line was captured before assertion");
        };

        let actual_log = last_log.trim();

        ensure_eq(&actual_log, &expected, "captured log line matches expected")
    }

    /// Checks that no log line has been captured since the previous assertion.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure`] if a log line was captured.
    pub fn try_assert_not_logged(&self) -> Result<(), TestFailure> {
        if self.take_last_log().is_some() {
            return ensure(false, "no log line was captured before assertion");
        }

        Ok(())
    }

    /// Takes the last captured log line, leaving the capture slot empty.
    fn take_last_log(&self) -> Option<String> {
        self.state.last_log.lock().take()
    }
}
