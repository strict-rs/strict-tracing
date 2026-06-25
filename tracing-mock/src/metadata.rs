use std::fmt;

use tracing_core::Metadata;

use crate::failure::{ExpectationError, ExpectationResult};

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
    pub(super) name: Option<String>,
    /// Expected metadata level.
    pub(super) level: Option<tracing::Level>,
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
    ///
    pub(super) fn check(
        &self,
        actual: &Metadata<'_>,
        ctx: impl fmt::Display,
        subscriber_name: &str,
    ) -> ExpectationResult {
        if let Some(ref expected_name) = self.name {
            let actual_name = actual.name();
            if expected_name != actual_name {
                return Err(ExpectationError::from_args(format_args!(
                    "\n[{subscriber_name}] expected {ctx} named `{expected_name}`,\n\
                    [{subscriber_name}] but got one named `{actual_name}` instead."
                )));
            }
        }

        if let Some(ref expected_level) = self.level {
            let actual_level = actual.level();
            if expected_level != actual_level {
                return Err(ExpectationError::from_args(format_args!(
                    "\n[{subscriber_name}] expected {ctx} at level `{expected_level}`,\n\
                    [{subscriber_name}] but got one at level `{actual_level}` instead.",
                    expected_level = display_level(*expected_level),
                    actual_level = display_level(*actual_level)
                )));
            }
        }

        if let Some(ref expected_target) = self.target {
            let actual_target = actual.target();
            if expected_target != actual_target {
                return Err(ExpectationError::from_args(format_args!(
                    "\n[{subscriber_name}] expected {ctx} with target `{expected_target}`,\n\
                    [{subscriber_name}] but got one with target `{actual_target}` instead."
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
