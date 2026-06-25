//! Example binary for tracing workspace checks.
#![cfg(test)]
use tracing_attributes::instrument;

const ROOT_MODULE_PATH: &str = module_path!();

#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "target fixture remains a named function item so default target metadata can be asserted"
)]
fn default_target() {}

#[instrument(target = "my_target")]
#[allow(
    clippy::single_call_fn,
    reason = "target fixture remains a named function item so custom target metadata can be asserted"
)]
fn custom_target() {}

mod nested_target_tests {
    use super::{ROOT_MODULE_PATH, custom_target, default_target};
    use strict_test_support::{TestFailure, ensure_ok};
    use tracing::subscriber::with_default;
    use tracing_attributes::instrument;
    use tracing_mock::*;

    const MODULE_PATH: &str = module_path!();

    #[instrument]
    #[allow(
        clippy::single_call_fn,
        reason = "nested target fixture remains a named function item so module-path metadata can be asserted"
    )]
    fn nested_default_target() {}

    #[instrument(target = "my_other_target")]
    #[allow(
        clippy::single_call_fn,
        reason = "nested target fixture remains a named function item so custom module target metadata can be asserted"
    )]
    fn nested_custom_target() {}

    #[test]
    fn default_targets() -> Result<(), TestFailure> {
        let (subscriber, handle) = subscriber::mock()
            .new_span(
                expect::span()
                    .named("default_target")
                    .with_target(ROOT_MODULE_PATH),
            )
            .enter(
                expect::span()
                    .named("default_target")
                    .with_target(ROOT_MODULE_PATH),
            )
            .exit(
                expect::span()
                    .named("default_target")
                    .with_target(ROOT_MODULE_PATH),
            )
            .new_span(
                expect::span()
                    .named("nested_default_target")
                    .with_target(MODULE_PATH),
            )
            .enter(
                expect::span()
                    .named("nested_default_target")
                    .with_target(MODULE_PATH),
            )
            .exit(
                expect::span()
                    .named("nested_default_target")
                    .with_target(MODULE_PATH),
            )
            .only()
            .run_with_handle();

        with_default(subscriber, || {
            default_target();
            nested_default_target();
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn custom_targets() -> Result<(), TestFailure> {
        let (subscriber, handle) = subscriber::mock()
            .new_span(
                expect::span()
                    .named("custom_target")
                    .with_target("my_target"),
            )
            .enter(
                expect::span()
                    .named("custom_target")
                    .with_target("my_target"),
            )
            .exit(
                expect::span()
                    .named("custom_target")
                    .with_target("my_target"),
            )
            .new_span(
                expect::span()
                    .named("nested_custom_target")
                    .with_target("my_other_target"),
            )
            .enter(
                expect::span()
                    .named("nested_custom_target")
                    .with_target("my_other_target"),
            )
            .exit(
                expect::span()
                    .named("nested_custom_target")
                    .with_target("my_other_target"),
            )
            .only()
            .run_with_handle();

        with_default(subscriber, || {
            custom_target();
            nested_custom_target();
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
