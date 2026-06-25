//! Compile coverage for `MockHandle` completion helpers.
#![cfg(test)]

#[test]
fn pass() -> Result<(), strict_test_support::TestFailure> {
    strict_test_support::ensure_compiles(
        "tests/ui/pass/*.rs",
        "mock handle pass UI fixtures compile",
    )
}

#[test]
fn compile_fail() -> Result<(), strict_test_support::TestFailure> {
    strict_test_support::ensure_compile_fail(
        "tests/ui/fail/*.rs",
        "mock handle fail UI fixtures match stderr",
    )
}
