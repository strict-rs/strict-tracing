//! Example binary for tracing workspace checks.
#![cfg(test)]
use strict_test_support::{TestFailure, ensure_ok};
use tracing::{Id, Level, Span, subscriber::with_default};
use tracing_attributes::instrument;
use tracing_mock::{expect, subscriber};
use tracing_test::block_on_future;

#[instrument(follows_from = causes, skip(causes))]
#[allow(
    clippy::single_call_fn,
    reason = "follows_from fixture remains a named function item so generated span links can be asserted"
)]
fn with_follows_from_sync(causes: impl IntoIterator<Item = impl Into<Option<Id>>>) {}

#[instrument(follows_from = causes, skip(causes))]
#[allow(
    clippy::single_call_fn,
    reason = "async follows_from fixture remains a named function item so generated span links can be asserted"
)]
async fn with_follows_from_async(causes: impl IntoIterator<Item = impl Into<Option<Id>>>) {}

#[instrument(follows_from = [&Span::current()])]
#[allow(
    clippy::single_call_fn,
    reason = "current-span follows_from fixture remains named so the macro expression form is asserted"
)]
fn follows_from_current() {}

#[test]
fn follows_from_sync_test() -> Result<(), TestFailure> {
    let cause_a = expect::span().named("cause_a");
    let cause_b = expect::span().named("cause_b");
    let cause_c = expect::span().named("cause_c");
    let consequence = expect::span().named("with_follows_from_sync");

    let (subscriber, handle) = subscriber::mock()
        .new_span(cause_a.clone())
        .new_span(cause_b.clone())
        .new_span(cause_c.clone())
        .new_span(consequence.clone())
        .follows_from(consequence.clone(), cause_a)
        .follows_from(consequence.clone(), cause_b)
        .follows_from(consequence.clone(), cause_c)
        .enter(consequence.clone())
        .exit(consequence)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        let runtime_cause_a = tracing::span!(Level::TRACE, "cause_a");
        let runtime_cause_b = tracing::span!(Level::TRACE, "cause_b");
        let runtime_cause_c = tracing::span!(Level::TRACE, "cause_c");

        with_follows_from_sync(&[runtime_cause_a, runtime_cause_b, runtime_cause_c]);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn follows_from_async_test() -> Result<(), TestFailure> {
    let cause_a = expect::span().named("cause_a");
    let cause_b = expect::span().named("cause_b");
    let cause_c = expect::span().named("cause_c");
    let consequence = expect::span().named("with_follows_from_async");

    let (subscriber, handle) = subscriber::mock()
        .new_span(cause_a.clone())
        .new_span(cause_b.clone())
        .new_span(cause_c.clone())
        .new_span(consequence.clone())
        .follows_from(consequence.clone(), cause_a)
        .follows_from(consequence.clone(), cause_b)
        .follows_from(consequence.clone(), cause_c)
        .enter(consequence.clone())
        .exit(consequence.clone())
        .enter(consequence.clone())
        .exit(consequence)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async {
            let runtime_cause_a = tracing::span!(Level::TRACE, "cause_a");
            let runtime_cause_b = tracing::span!(Level::TRACE, "cause_b");
            let runtime_cause_c = tracing::span!(Level::TRACE, "cause_c");

            with_follows_from_async(&[runtime_cause_a, runtime_cause_b, runtime_cause_c]).await;
        });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn follows_from_current_test() -> Result<(), TestFailure> {
    let cause = expect::span().named("cause");
    let consequence = expect::span().named("follows_from_current");

    let (subscriber, handle) = subscriber::mock()
        .new_span(cause.clone())
        .enter(cause.clone())
        .new_span(consequence.clone())
        .follows_from(consequence.clone(), cause.clone())
        .enter(consequence.clone())
        .exit(consequence)
        .exit(cause)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        tracing::span!(Level::TRACE, "cause").in_scope(follows_from_current);
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
