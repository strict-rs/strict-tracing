//! Compile coverage for subscriber initialization APIs.
#![cfg(test)]

#[test]
fn ui() -> Result<(), trybuild::TryBuildError> {
  let mut cases = trybuild::TestCases::new();
  cases.pass("tests/ui/pass/*.rs");
  cases.compile_fail("tests/ui/fail/*.rs");
  cases.run()
}
