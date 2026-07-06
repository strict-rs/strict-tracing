use std::fmt;

use tracing_core::Metadata;

use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;

/// Formats `tracing::Level` like its historical debug output.
pub(crate) fn display_level(level: tracing::Level) -> &'static str {
  if level == tracing::Level::ERROR {
    "Level(Error)"
  } else if level == tracing::Level::WARN {
    "Level(Warn)"
  } else if level == tracing::Level::INFO {
    "Level(Info)"
  } else if level == tracing::Level::DEBUG {
    "Level(Debug)"
  } else {
    "Level(Trace)"
  }
}

/// Metadata expectations shared by expected spans and events.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub(crate) struct ExpectedMetadata {
  /// Expected metadata name.
  pub(super) name:   Option<String>,
  /// Expected metadata level.
  pub(super) level:  Option<tracing::Level>,
  /// Expected metadata target.
  pub(super) target: Option<String>,
}

impl ExpectedMetadata {
  /// Checks the given metadata against this expected metadata and errors if
  /// there is a mismatch.
  ///
  /// The context `ctx` should fit into the followint sentence:
  ///
  /// > expected {ctx} named `expected_name`, but got one named `actual_name`
  ///
  /// Examples could be:
  /// * a new span
  /// * to enter a span
  /// * an event
  pub(super) fn check(&self, actual: &Metadata<'_>, ctx: impl fmt::Display, subscriber_name: &str) -> ExpectationResult {
    if let Some(ref expected_name) = self.name {
      let actual_name = actual.name();
      if expected_name != actual_name {
        return Err(ExpectationError::from_args(format_args!(
          "\n[{subscriber_name}] expected {ctx} named `{expected_name}`,\n[{subscriber_name}] but got one named `{actual_name}` instead."
        )));
      }
    }

    if let Some(ref expected_level) = self.level {
      let actual_level = actual.level();
      if expected_level != actual_level {
        return Err(ExpectationError::from_args(format_args!(
          "\n[{subscriber_name}] expected {ctx} at level `{expected_level}`,\n[{subscriber_name}] but got one at level `{actual_level}` \
           instead.",
          expected_level = display_level(*expected_level),
          actual_level = display_level(*actual_level)
        )));
      }
    }

    if let Some(ref expected_target) = self.target {
      let actual_target = actual.target();
      if expected_target != actual_target {
        return Err(ExpectationError::from_args(format_args!(
          "\n[{subscriber_name}] expected {ctx} with target `{expected_target}`,\n[{subscriber_name}] but got one with target \
           `{actual_target}` instead."
        )));
      }
    }

    Ok(())
  }

  /// Returns `true` when at least one metadata field is constrained.
  pub(super) const fn has_expectations(&self) -> bool {
    self.name.is_some() || self.level.is_some() || self.target.is_some()
  }
}

impl fmt::Display for ExpectedMetadata {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(ref name) = self.name {
      write!(f, " named `{name}`")?;
    }

    if let Some(ref level) = self.level {
      write!(f, " at the `{}` level", display_level(*level))?;
    }

    if let Some(ref target) = self.target {
      write!(f, " with target `{target}`")?;
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::Interest;
  use tracing_core::Level;
  use tracing_core::Metadata;
  use tracing_core::callsite::Callsite;
  use tracing_core::metadata;
  use tracing_core::metadata::Kind;

  use super::ExpectedMetadata;

  struct MetadataTestCallsite;

  static METADATA_TEST_CALLSITE: MetadataTestCallsite = MetadataTestCallsite;
  static METADATA_TEST: Metadata<'static> = metadata! {
      name: "metadata_test",
      target: "metadata_target",
      level: Level::INFO,
      fields: &[],
      callsite: &METADATA_TEST_CALLSITE,
      kind: Kind::EVENT
  };

  impl Callsite for MetadataTestCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      &METADATA_TEST
    }
  }

  #[test]
  fn metadata_expectations_accept_matching_metadata() -> Result<(), TestFailure> {
    let expected = ExpectedMetadata {
      name:   Some("metadata_test".to_owned()),
      level:  Some(Level::INFO),
      target: Some("metadata_target".to_owned()),
    };

    ensure_ok(
      expected.check(&METADATA_TEST, "an event", "metadata-test"),
      "matching metadata is accepted",
    )
  }

  #[test]
  fn metadata_expectations_reject_mismatching_name_level_and_target() -> Result<(), TestFailure> {
    let wrong_name = ExpectedMetadata {
      name: Some("other_name".to_owned()),
      ..ExpectedMetadata::default()
    };
    let wrong_level = ExpectedMetadata {
      level: Some(Level::ERROR),
      ..ExpectedMetadata::default()
    };
    let wrong_target = ExpectedMetadata {
      target: Some("other_target".to_owned()),
      ..ExpectedMetadata::default()
    };

    ensure(
      wrong_name.check(&METADATA_TEST, "an event", "metadata-test").is_err(),
      "wrong name is rejected",
    )?;
    ensure(
      wrong_level.check(&METADATA_TEST, "an event", "metadata-test").is_err(),
      "wrong level is rejected",
    )?;
    ensure(
      wrong_target.check(&METADATA_TEST, "an event", "metadata-test").is_err(),
      "wrong target is rejected",
    )
  }
}
