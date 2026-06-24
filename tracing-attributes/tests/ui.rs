//! Example binary for tracing workspace checks.

// Only test on stable, since UI tests are bound to change over time

#[rustversion::stable]
#[test]
fn pass() -> Result<(), strict_test_support::TestFailure> {
    strict_test_support::ensure_compiles(
        "tests/ui/pass/*.rs",
        "instrument pass UI fixtures compile",
    )
}

#[rustversion::stable]
#[test]
fn compile_fail() -> Result<(), strict_test_support::TestFailure> {
    strict_test_support::ensure_compile_fail(
        "tests/ui/fail/*.rs",
        "instrument fail UI fixtures match stderr",
    )
}
