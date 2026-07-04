//! Compile coverage for subscriber initialization APIs.
#![cfg(test)]

#[test]
fn pass() -> Result<(), strict_test_support::TestFailure> {
  strict_test_support::ensure_compiles("tests/ui/pass/*.rs", "subscriber init pass UI fixtures compile")
}

#[test]
fn compile_fail() -> Result<(), strict_test_support::TestFailure> {
  strict_test_support::ensure_compile_fail("tests/ui/fail/*.rs", "subscriber init fail UI fixtures match stderr")
}
