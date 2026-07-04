//! Trace verbosity level filtering.
//!
//! # Compile time filters
//!
//! Trace verbosity levels can be statically disabled at compile time via Cargo
//! features, similar to the [`log` crate]. Trace instrumentation at disabled
//! levels will be skipped and will not even be present in the resulting binary
//! unless the verbosity level is specified dynamically. This level is
//! configured separately for release and debug builds. The features are:
//!
//! * `max_level_off`
//! * `max_level_error`
//! * `max_level_warn`
//! * `max_level_info`
//! * `max_level_debug`
//! * `max_level_trace`
//! * `release_max_level_off`
//! * `release_max_level_error`
//! * `release_max_level_warn`
//! * `release_max_level_info`
//! * `release_max_level_debug`
//! * `release_max_level_trace`
//!
//! These features control the value of the `STATIC_MAX_LEVEL` constant. The
//! instrumentation macros check this value before recording an event or
//! constructing a span. By default, no levels are disabled.
//!
//! Cargo features are additive, so if more than one static max level feature is
//! enabled in the same profile, the most permissive enabled level is used.
//! For example, enabling both `max_level_off` and `max_level_info` resolves to
//! `INFO`, and enabling all `max_level_*` features resolves to `TRACE`.
//!
//! For example, a crate can disable trace level instrumentation in debug builds
//! and trace, debug, and info level instrumentation in release builds with the
//! following configuration:
//!
//! ```toml
//! [dependencies]
//! tracing = { version = "0.1", features = ["max_level_debug", "release_max_level_warn"] }
//! ```
//! ## Notes
//!
//! Please note that `tracing`'s static max level features do *not* control the
//! [`log`] records that may be emitted when [`tracing`'s "log" feature flag][f] is
//! enabled. This is to allow `tracing` to be disabled entirely at compile time
//! while still emitting `log` records --- such as when a library using
//! `tracing` is used by an application using `log` that doesn't want to
//! generate any `tracing`-related code, but does want to collect `log` records.
//!
//! This means that if the "log" feature is in use, some code may be generated
//! for `log` records emitted by disabled `tracing` events. If this is not
//! desirable, `log` records may be disabled separately using [`log`'s static
//! max level features][`log` crate].
//!
//! [`log`]: https://docs.rs/log/
//! [`log` crate]: https://docs.rs/log/latest/log/#compile-time-filters
//! [f]: https://docs.rs/tracing/latest/tracing/#emitting-log-records
pub use tracing_core::LevelFilter;
pub use tracing_core::metadata::ParseLevelFilterError;

/// The statically configured maximum trace level.
///
/// See the [module-level documentation] for information on how to configure
/// this.
///
/// This value is checked by the `event!` and `span!` macros. Code that
/// manually constructs events or spans via the `Event::record` function or
/// `Span` constructors should compare the level against this value to
/// determine if those spans or events are enabled.
///
/// [module-level documentation]: self#compile-time-filters
pub const STATIC_MAX_LEVEL: LevelFilter = {
  if !cfg!(debug_assertions)
    && (cfg!(feature = "release_max_level_off")
      || cfg!(feature = "release_max_level_error")
      || cfg!(feature = "release_max_level_warn")
      || cfg!(feature = "release_max_level_info")
      || cfg!(feature = "release_max_level_debug")
      || cfg!(feature = "release_max_level_trace"))
  {
    select_max_level(
      FeatureState::from_cfg(cfg!(feature = "release_max_level_off")),
      FeatureState::from_cfg(cfg!(feature = "release_max_level_error")),
      FeatureState::from_cfg(cfg!(feature = "release_max_level_warn")),
      FeatureState::from_cfg(cfg!(feature = "release_max_level_info")),
      FeatureState::from_cfg(cfg!(feature = "release_max_level_debug")),
      FeatureState::from_cfg(cfg!(feature = "release_max_level_trace")),
    )
  } else {
    select_max_level(
      FeatureState::from_cfg(cfg!(feature = "max_level_off")),
      FeatureState::from_cfg(cfg!(feature = "max_level_error")),
      FeatureState::from_cfg(cfg!(feature = "max_level_warn")),
      FeatureState::from_cfg(cfg!(feature = "max_level_info")),
      FeatureState::from_cfg(cfg!(feature = "max_level_debug")),
      FeatureState::from_cfg(cfg!(feature = "max_level_trace")),
    )
  }
};

/// Whether a static max-level feature is enabled.
#[derive(Clone, Copy)]
enum FeatureState {
  /// The feature is enabled.
  Enabled,
  /// The feature is disabled.
  Disabled,
}

impl FeatureState {
  /// Convert the result of `cfg!(feature = ...)` into a named state.
  const fn from_cfg(enabled: bool) -> Self {
    if enabled { Self::Enabled } else { Self::Disabled }
  }

  /// Return whether this feature state is enabled.
  const fn is_enabled(self) -> bool {
    matches!(self, Self::Enabled)
  }
}

/// Choose the most permissive enabled static max-level feature.
const fn select_max_level(
  off: FeatureState,
  error: FeatureState,
  warn: FeatureState,
  info: FeatureState,
  debug: FeatureState,
  trace: FeatureState,
) -> LevelFilter {
  if trace.is_enabled() {
    LevelFilter::TRACE
  } else if debug.is_enabled() {
    LevelFilter::DEBUG
  } else if info.is_enabled() {
    LevelFilter::INFO
  } else if warn.is_enabled() {
    LevelFilter::WARN
  } else if error.is_enabled() {
    LevelFilter::ERROR
  } else if off.is_enabled() {
    LevelFilter::OFF
  } else {
    LevelFilter::TRACE
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_eq;

  use super::FeatureState;
  use super::LevelFilter;
  use super::select_max_level;

  const DISABLED: FeatureState = FeatureState::Disabled;
  const ENABLED: FeatureState = FeatureState::Enabled;

  #[test]
  fn select_max_level_defaults_to_trace() -> Result<(), TestFailure> {
    ensure_eq(
      &select_max_level(DISABLED, DISABLED, DISABLED, DISABLED, DISABLED, DISABLED),
      &LevelFilter::TRACE,
      "no max-level feature defaults to TRACE",
    )
  }

  #[test]
  fn select_max_level_preserves_single_restrictive_features() -> Result<(), TestFailure> {
    ensure_eq(
      &select_max_level(ENABLED, DISABLED, DISABLED, DISABLED, DISABLED, DISABLED),
      &LevelFilter::OFF,
      "max_level_off maps to OFF",
    )?;
    ensure_eq(
      &select_max_level(DISABLED, ENABLED, DISABLED, DISABLED, DISABLED, DISABLED),
      &LevelFilter::ERROR,
      "max_level_error maps to ERROR",
    )?;
    ensure_eq(
      &select_max_level(DISABLED, DISABLED, ENABLED, DISABLED, DISABLED, DISABLED),
      &LevelFilter::WARN,
      "max_level_warn maps to WARN",
    )?;
    ensure_eq(
      &select_max_level(DISABLED, DISABLED, DISABLED, ENABLED, DISABLED, DISABLED),
      &LevelFilter::INFO,
      "max_level_info maps to INFO",
    )?;
    ensure_eq(
      &select_max_level(DISABLED, DISABLED, DISABLED, DISABLED, ENABLED, DISABLED),
      &LevelFilter::DEBUG,
      "max_level_debug maps to DEBUG",
    )
  }

  #[test]
  fn select_max_level_uses_most_permissive_enabled_feature() -> Result<(), TestFailure> {
    ensure_eq(
      &select_max_level(ENABLED, DISABLED, DISABLED, ENABLED, DISABLED, DISABLED),
      &LevelFilter::INFO,
      "more permissive INFO wins over OFF",
    )?;
    ensure_eq(
      &select_max_level(ENABLED, ENABLED, ENABLED, ENABLED, ENABLED, DISABLED),
      &LevelFilter::DEBUG,
      "more permissive DEBUG wins over lower levels",
    )?;
    ensure_eq(
      &select_max_level(ENABLED, ENABLED, ENABLED, ENABLED, ENABLED, ENABLED),
      &LevelFilter::TRACE,
      "TRACE is the most permissive static level",
    )
  }
}
