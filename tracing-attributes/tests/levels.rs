//! Example binary for tracing workspace checks.
#![cfg(test)]

use strict_test_support::{TestFailure, ensure_ok};
use tracing::Level;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::*;

#[test]
fn named_levels() -> Result<(), TestFailure> {
    #[instrument(level = "trace")]
    fn trace() {}

    #[instrument(level = "Debug")]
    fn debug() {}

    #[instrument(level = "INFO")]
    fn info() {}

    #[instrument(level = "WARn")]
    fn warn() {}

    #[instrument(level = "eRrOr")]
    fn error() {}
    let (subscriber, handle) = subscriber::mock()
        .new_span(expect::span().named("trace").at_level(Level::TRACE))
        .enter(expect::span().named("trace").at_level(Level::TRACE))
        .exit(expect::span().named("trace").at_level(Level::TRACE))
        .new_span(expect::span().named("debug").at_level(Level::DEBUG))
        .enter(expect::span().named("debug").at_level(Level::DEBUG))
        .exit(expect::span().named("debug").at_level(Level::DEBUG))
        .new_span(expect::span().named("info").at_level(Level::INFO))
        .enter(expect::span().named("info").at_level(Level::INFO))
        .exit(expect::span().named("info").at_level(Level::INFO))
        .new_span(expect::span().named("warn").at_level(Level::WARN))
        .enter(expect::span().named("warn").at_level(Level::WARN))
        .exit(expect::span().named("warn").at_level(Level::WARN))
        .new_span(expect::span().named("error").at_level(Level::ERROR))
        .enter(expect::span().named("error").at_level(Level::ERROR))
        .exit(expect::span().named("error").at_level(Level::ERROR))
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        trace();
        debug();
        info();
        warn();
        error();
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn numeric_levels() -> Result<(), TestFailure> {
    #[instrument(level = 1)]
    fn trace() {}

    #[instrument(level = 2)]
    fn debug() {}

    #[instrument(level = 3)]
    fn info() {}

    #[instrument(level = 4)]
    fn warn() {}

    #[instrument(level = 5)]
    fn error() {}
    let (subscriber, handle) = subscriber::mock()
        .new_span(expect::span().named("trace").at_level(Level::TRACE))
        .enter(expect::span().named("trace").at_level(Level::TRACE))
        .exit(expect::span().named("trace").at_level(Level::TRACE))
        .new_span(expect::span().named("debug").at_level(Level::DEBUG))
        .enter(expect::span().named("debug").at_level(Level::DEBUG))
        .exit(expect::span().named("debug").at_level(Level::DEBUG))
        .new_span(expect::span().named("info").at_level(Level::INFO))
        .enter(expect::span().named("info").at_level(Level::INFO))
        .exit(expect::span().named("info").at_level(Level::INFO))
        .new_span(expect::span().named("warn").at_level(Level::WARN))
        .enter(expect::span().named("warn").at_level(Level::WARN))
        .exit(expect::span().named("warn").at_level(Level::WARN))
        .new_span(expect::span().named("error").at_level(Level::ERROR))
        .enter(expect::span().named("error").at_level(Level::ERROR))
        .exit(expect::span().named("error").at_level(Level::ERROR))
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        trace();
        debug();
        info();
        warn();
        error();
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn enum_levels() -> Result<(), TestFailure> {
    #[instrument(level = Level::TRACE)]
    fn trace() {}

    #[instrument(level = Level::DEBUG)]
    fn debug() {}

    #[instrument(level = Level::INFO)]
    fn info() {}

    #[instrument(level = Level::WARN)]
    fn warn() {}

    #[instrument(level = Level::ERROR)]
    fn error() {}
    let (subscriber, handle) = subscriber::mock()
        .new_span(expect::span().named("trace").at_level(Level::TRACE))
        .enter(expect::span().named("trace").at_level(Level::TRACE))
        .exit(expect::span().named("trace").at_level(Level::TRACE))
        .new_span(expect::span().named("debug").at_level(Level::DEBUG))
        .enter(expect::span().named("debug").at_level(Level::DEBUG))
        .exit(expect::span().named("debug").at_level(Level::DEBUG))
        .new_span(expect::span().named("info").at_level(Level::INFO))
        .enter(expect::span().named("info").at_level(Level::INFO))
        .exit(expect::span().named("info").at_level(Level::INFO))
        .new_span(expect::span().named("warn").at_level(Level::WARN))
        .enter(expect::span().named("warn").at_level(Level::WARN))
        .exit(expect::span().named("warn").at_level(Level::WARN))
        .new_span(expect::span().named("error").at_level(Level::ERROR))
        .enter(expect::span().named("error").at_level(Level::ERROR))
        .exit(expect::span().named("error").at_level(Level::ERROR))
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        trace();
        debug();
        info();
        warn();
        error();
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
