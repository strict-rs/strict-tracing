//! Example binary for tracing workspace checks.
#![cfg(test)]

use tracing_attributes::instrument;

#[deny(unfulfilled_lint_expectations)]
#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "reachability fixture remains a named instrumented function so dead-code behavior is asserted"
)]
fn unused() {}

#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "async reachability fixture remains a named instrumented function so dead-code behavior is asserted"
)]
async fn unused_async() {}

#[test]
fn instrumented_functions_are_reachable() {
    unused();
    tracing_test::block_on_future(unused_async());
}
