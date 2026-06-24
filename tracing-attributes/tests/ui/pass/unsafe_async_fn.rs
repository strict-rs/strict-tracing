//! This program verifies that `#[instrument]` accepts unsafe async functions.
#![allow(
    unsafe_code,
    reason = "compile-pass fixture verifies #[instrument] accepts unsafe async functions"
)]

#[tracing::instrument]
async unsafe fn test_async_unsafe_fn_empty() {}

fn main() {}
