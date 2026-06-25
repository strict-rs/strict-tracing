//! Example binary for tracing workspace checks.
#![cfg(test)]

use std::convert::TryFrom as _;
use std::num::TryFromIntError;

use strict_test_support::{TestFailure, ensure, ensure_ok};
use tracing::{
    Level,
    field::{debug, display},
    subscriber::with_default,
};
use tracing_attributes::instrument;
use tracing_mock::{expect, subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_test::block_on_future;

#[instrument(ret)]
#[allow(
    clippy::single_call_fn,
    reason = "ret fixture remains a named instrumented function so default return events can be asserted"
)]
fn ret() -> i32 {
    42
}

#[instrument(target = "my_target", ret)]
#[allow(
    clippy::single_call_fn,
    reason = "ret fixture remains a named instrumented function so target-filtered return events can be asserted"
)]
fn ret_with_target() -> i32 {
    42
}

#[test]
fn test() -> Result<(), TestFailure> {
    let span = expect::span().named("ret");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn test_custom_target() -> Result<(), TestFailure> {
    let filter: EnvFilter = ensure_ok("my_target=info".parse(), "filter should parse")?;
    let span = expect::span()
        .named("ret_with_target")
        .with_target("my_target");

    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::INFO)
                .with_target("my_target"),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let filtered_subscriber = subscriber.with(filter);

    let _result = with_default(filtered_subscriber, ret_with_target);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(level = "warn", ret)]
#[allow(
    clippy::single_call_fn,
    reason = "ret fixture remains a named instrumented function so span-level return events can be asserted"
)]
fn ret_warn() -> i32 {
    42
}

#[test]
fn test_warn() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_warn");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::WARN),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret_warn);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(ret)]
#[allow(
    clippy::single_call_fn,
    reason = "mutable ret fixture remains a named instrumented function so return events preserve mutation behavior"
)]
fn ret_mut(arg: &mut i32) -> i32 {
    *arg = (*arg).saturating_mul(2);
    tracing::info!(a = ?arg);
    *arg
}

#[test]
fn test_mut() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_mut");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("a").with_value(&display(2)))
                .at_level(Level::INFO),
        )
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(2)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, || ret_mut(&mut 1));
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(ret)]
#[allow(
    clippy::single_call_fn,
    reason = "async ret fixture remains a named instrumented function so awaited return events can be asserted"
)]
async fn ret_async() -> i32 {
    42
}

#[test]
fn test_async() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_async");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, || block_on_future(async { ret_async().await }));
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(ret)]
#[allow(
    clippy::single_call_fn,
    reason = "impl Trait ret fixture remains a named instrumented function so opaque return types are asserted"
)]
fn ret_impl_type() -> impl Copy {
    42
}

#[test]
fn test_impl_type() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_impl_type");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret_impl_type);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(ret(Display))]
#[allow(
    clippy::single_call_fn,
    reason = "display ret fixture remains a named instrumented function so display return formatting is asserted"
)]
fn ret_display() -> i32 {
    42
}

#[test]
fn test_dbg() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_display");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&display(42)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret_display);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(err, ret)]
#[allow(
    clippy::single_call_fn,
    reason = "ret-and-err fixture remains a named instrumented function so error precedence is asserted"
)]
fn ret_and_err() -> Result<u8, TryFromIntError> {
    u8::try_from(1234)
}

#[test]
fn test_ret_and_err() -> Result<(), TestFailure> {
    let Err(expected_error) = u8::try_from(1234) else {
        return ensure(false, "1234 should not fit in u8");
    };
    let span = expect::span().named("ret_and_err");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(
                    expect::field("error")
                        .with_value(&display(expected_error))
                        .only(),
                )
                .at_level(Level::ERROR),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, || ret_and_err().ok());
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(err, ret)]
#[allow(
    clippy::single_call_fn,
    reason = "ret-and-ok fixture remains a named instrumented function so successful result returns are asserted"
)]
fn ret_and_ok() -> Result<u8, TryFromIntError> {
    u8::try_from(123)
}

#[test]
fn test_ret_and_ok() -> Result<(), TestFailure> {
    let expected_return = ensure_ok(u8::try_from(123), "123 should fit in u8")?;
    let span = expect::span().named("ret_and_ok");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(
                    expect::field("return")
                        .with_value(&debug(expected_return))
                        .only(),
                )
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, || ret_and_ok().ok());
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(level = "warn", ret(level = "info"))]
#[allow(
    clippy::single_call_fn,
    reason = "ret fixture remains a named instrumented function so ret level overrides span level"
)]
fn ret_warn_info() -> i32 {
    42
}

#[test]
fn test_warn_info() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_warn_info").at_level(Level::WARN);
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::INFO),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret_warn_info);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[instrument(ret(level = "warn", Debug))]
#[allow(
    clippy::single_call_fn,
    reason = "debug ret fixture remains a named instrumented function so ret formatting and level override are asserted"
)]
fn ret_dbg_warn() -> i32 {
    42
}

#[test]
fn test_dbg_warn() -> Result<(), TestFailure> {
    let span = expect::span().named("ret_dbg_warn").at_level(Level::INFO);
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .event(
            expect::event()
                .with_fields(expect::field("return").with_value(&debug(42)))
                .at_level(Level::WARN),
        )
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, ret_dbg_warn);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
