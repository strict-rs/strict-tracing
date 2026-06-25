//! Tests formatter max level hints.
#![cfg(feature = "fmt")]

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure, ensure_eq};
    use tracing_subscriber::filter::LevelFilter;

    #[test]
    fn fmt_sets_max_level_hint() -> Result<(), TestFailure> {
        let init_result = tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .try_init();
        ensure(init_result.is_ok(), "fmt try_init succeeds")?;
        ensure_eq(
            &LevelFilter::current(),
            &LevelFilter::DEBUG,
            "fmt init updates the current max level hint",
        )
    }
}
