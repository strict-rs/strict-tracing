//! Example binary for tracing workspace checks.

use tracing_attributes::instrument;

#[deny(unfulfilled_lint_expectations)]
#[instrument]
fn unused() {}

#[instrument]
async fn unused_async() {}
