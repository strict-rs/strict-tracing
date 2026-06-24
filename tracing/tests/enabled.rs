#![cfg(feature = "std")]
use tracing::{
    Level,
    level_filters::{LevelFilter, STATIC_MAX_LEVEL},
};
use tracing_mock::*;

fn statically_enabled(level: Level) -> bool {
    level <= STATIC_MAX_LEVEL
}

fn expected_static_max_level() -> LevelFilter {
    if !cfg!(debug_assertions) && release_max_level_configured() {
        expected_release_max_level()
    } else {
        expected_debug_max_level()
    }
}

fn release_max_level_configured() -> bool {
    cfg!(feature = "release_max_level_off")
        || cfg!(feature = "release_max_level_error")
        || cfg!(feature = "release_max_level_warn")
        || cfg!(feature = "release_max_level_info")
        || cfg!(feature = "release_max_level_debug")
        || cfg!(feature = "release_max_level_trace")
}

fn expected_release_max_level() -> LevelFilter {
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

fn expected_debug_max_level() -> LevelFilter {
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
fn static_max_level_matches_active_feature_set() {
    assert_eq!(STATIC_MAX_LEVEL, expected_static_max_level());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[test]
fn level_and_target() {
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

    let _guard = tracing::subscriber::set_default(subscriber);

    assert_eq!(
        tracing::enabled!(target: "debug_module", Level::DEBUG),
        statically_enabled(Level::DEBUG)
    );
    assert_eq!(
        tracing::enabled!(Level::ERROR),
        statically_enabled(Level::ERROR)
    );
    assert!(!tracing::enabled!(Level::DEBUG));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[test]
fn span_and_event() {
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

    let _guard = tracing::subscriber::set_default(subscriber);

    // Ensure that the `_event` and `_span` alternatives work correctly
    assert!(!tracing::event_enabled!(Level::TRACE));
    assert_eq!(
        tracing::event_enabled!(Level::DEBUG),
        statically_enabled(Level::DEBUG)
    );
    assert_eq!(
        tracing::span_enabled!(Level::TRACE),
        statically_enabled(Level::TRACE)
    );

    // target variants
    assert_eq!(
        tracing::span_enabled!(target: "debug_module", Level::DEBUG),
        statically_enabled(Level::DEBUG)
    );
    assert_eq!(
        tracing::event_enabled!(target: "debug_module", Level::DEBUG),
        statically_enabled(Level::DEBUG)
    );
}
