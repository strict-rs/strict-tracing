//! Example binary for tracing workspace checks.
#![cfg(test)]

// Only test on stable, since UI tests are bound to change over time

#[rustversion::stable]
#[test]
fn ui() -> Result<(), trybuild::TryBuildError> {
  let mut cases = trybuild::TestCases::new();
  cases.pass("tests/ui/pass/*.rs");
  cases.compile_fail("tests/ui/fail/*.rs");
  cases.run()
}
