//! Compile coverage for subscriber initialization APIs.
#![cfg(test)]

/// Checks fallible formatting initialization and rejects panicking initialization APIs.
#[test]
#[cfg(feature = "fmt")]
fn fmt_initialization_requires_fallible_apis() -> Result<(), trybuild::TryBuildError> {
  let mut cases = trybuild::TestCases::new();
  cases.pass("tests/ui/pass/try_init.rs");
  cases.compile_fail("tests/ui/fail/fmt_builder_init_removed.rs");
  cases.compile_fail("tests/ui/fail/fmt_init_removed.rs");
  cases.run()
}

/// Rejects panicking registry initialization through the extension trait.
#[test]
#[cfg(feature = "registry")]
fn registry_initialization_requires_fallible_apis() -> Result<(), trybuild::TryBuildError> {
  let mut cases = trybuild::TestCases::new();
  cases.compile_fail("tests/ui/fail/subscriber_init_ext_init_removed.rs");
  cases.run()
}
