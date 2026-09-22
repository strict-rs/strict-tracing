//! Consumer-owned repository extension composition.
//!
//! Standard repository workflows execute through the installed `template`
//! binary. This crate compiles only the guarded local `x` registry.

use std::process::ExitCode;

pub mod extensions;

/// Run the guarded repository-specific extension surface.
#[must_use]
pub fn run() -> ExitCode {
  template_stask::run_with_extensions(extensions::commands())
}

#[cfg(test)]
mod tests {

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Retains the native registry failure.
    #[error(transparent)]
    Registry(#[from] strict_test_support::ResultFailure<template_stask::StaskError>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use template_core::cli::command::CommandSurface;

  use super::extensions;

  #[test]
  fn extension_registry_exposes_only_the_local_x_router() -> Result<(), TestError> {
    let command_set = ensure_ok(extensions::commands(), "the local extension registry must build")?;
    let descriptors = command_set.descriptors();
    ensure(
      descriptors.len() == 1
        && descriptors
          .first()
          .is_some_and(|descriptor| descriptor.name() == "x" && descriptor.surface() == CommandSurface::StaskExtension),
      "the consumer runner must expose only the local x extension surface",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
