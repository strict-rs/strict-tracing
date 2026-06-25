#![cfg(feature = "std")]
//! Enabled macro and level-filter integration coverage.

use tracing::{
    Level, enabled, event_enabled,
    level_filters::{LevelFilter, STATIC_MAX_LEVEL},
    span_enabled,
    subscriber::set_default,
};
use tracing_mock::subscriber;

#[cfg(test)]
mod tests {
    use super::*;
    use strict_test_support::{TestFailure, ensure, ensure_eq};

    fn statically_enabled(level: Level) -> bool {
        level <= STATIC_MAX_LEVEL
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keep the static max-level test oracle split by build profile"
    )]
    const fn expected_static_max_level() -> LevelFilter {
        if !cfg!(debug_assertions) && release_max_level_configured() {
            expected_release_max_level()
        } else {
            expected_debug_max_level()
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "name the release feature gate used by the static max-level oracle"
    )]
    const fn release_max_level_configured() -> bool {
        cfg!(feature = "release_max_level_off")
            || cfg!(feature = "release_max_level_error")
            || cfg!(feature = "release_max_level_warn")
            || cfg!(feature = "release_max_level_info")
            || cfg!(feature = "release_max_level_debug")
            || cfg!(feature = "release_max_level_trace")
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keep release static max-level expectations readable"
    )]
    const fn expected_release_max_level() -> LevelFilter {
        if cfg!(feature = "release_max_level_trace") {
            LevelFilter::TRACE
        } else if cfg!(feature = "release_max_level_debug") {
            LevelFilter::DEBUG
        } else if cfg!(feature = "release_max_level_info") {
            LevelFilter::INFO
        } else if cfg!(feature = "release_max_level_warn") {
            LevelFilter::WARN
        } else if cfg!(feature = "release_max_level_error") {
            LevelFilter::ERROR
        } else {
            LevelFilter::OFF
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "keep debug static max-level expectations readable"
    )]
    const fn expected_debug_max_level() -> LevelFilter {
        if cfg!(feature = "max_level_trace") {
            LevelFilter::TRACE
        } else if cfg!(feature = "max_level_debug") {
            LevelFilter::DEBUG
        } else if cfg!(feature = "max_level_info") {
            LevelFilter::INFO
        } else if cfg!(feature = "max_level_warn") {
            LevelFilter::WARN
        } else if cfg!(feature = "max_level_error") {
            LevelFilter::ERROR
        } else if cfg!(feature = "max_level_off") {
            LevelFilter::OFF
        } else {
            LevelFilter::TRACE
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn static_max_level_matches_active_feature_set() -> Result<(), TestFailure> {
        ensure_eq(
            &STATIC_MAX_LEVEL,
            &expected_static_max_level(),
            "static max level matches active feature set",
        )
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn level_and_target() -> Result<(), TestFailure> {
        let subscriber = subscriber::mock()
            .with_filter(|meta| {
                if meta.target() == "debug_module" {
                    meta.level() <= &Level::DEBUG
                } else {
                    meta.level() <= &Level::INFO
                }
            })
            .only()
            .run();

        let _guard = set_default(subscriber);

        ensure_eq(
            &enabled!(target: "debug_module", Level::DEBUG),
            &statically_enabled(Level::DEBUG),
            "targeted DEBUG enabled status matches static level",
        )?;
        ensure_eq(
            &enabled!(Level::ERROR),
            &statically_enabled(Level::ERROR),
            "ERROR enabled status matches static level",
        )?;
        ensure(
            !enabled!(Level::DEBUG),
            "untargeted DEBUG remains disabled by the subscriber filter",
        )
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn span_and_event() -> Result<(), TestFailure> {
        let subscriber = subscriber::mock()
            .with_filter(|meta| {
                if meta.target() == "debug_module" {
                    meta.level() <= &Level::DEBUG
                } else if meta.is_span() {
                    meta.level() <= &Level::TRACE
                } else if meta.is_event() {
                    meta.level() <= &Level::DEBUG
                } else {
                    meta.level() <= &Level::INFO
                }
            })
            .only()
            .run();

        let _guard = set_default(subscriber);

        // Ensure that the `_event` and `_span` alternatives work correctly.
        ensure(
            !event_enabled!(Level::TRACE),
            "TRACE event remains disabled by the subscriber filter",
        )?;
        ensure_eq(
            &event_enabled!(Level::DEBUG),
            &statically_enabled(Level::DEBUG),
            "DEBUG event enabled status matches static level",
        )?;
        ensure_eq(
            &span_enabled!(Level::TRACE),
            &statically_enabled(Level::TRACE),
            "TRACE span enabled status matches static level",
        )?;

        // Target variants.
        ensure_eq(
            &span_enabled!(target: "debug_module", Level::DEBUG),
            &statically_enabled(Level::DEBUG),
            "targeted DEBUG span enabled status matches static level",
        )?;
        ensure_eq(
            &event_enabled!(target: "debug_module", Level::DEBUG),
            &statically_enabled(Level::DEBUG),
            "targeted DEBUG event enabled status matches static level",
        )
    }
}
