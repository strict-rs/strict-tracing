//! Tests expectations for the parent made on [`ExpectedSpan`].
//!
//! The tests in this module completely cover the positive and negative cases
//! when expecting that a span is a contextual or explicit root or expecting
//! that a span has a specific contextual or explicit parent.
//!
//! [`ExpectedSpan`]: crate::span::ExpectedSpan
//!

use strict_test_support::{TestFailure, ensure_contains, ensure_ok, ensure_some};
use tracing::{Level, subscriber::with_default};
use tracing_mock::{expect, subscriber};

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_finished_error(
        handle: &subscriber::MockHandle,
        expected: &str,
    ) -> Result<(), TestFailure> {
        let error = ensure_some(
            handle.finished().err(),
            "mock expectations should return an ancestry mismatch",
        )?;
        ensure_contains(
            &error.to_string(),
            expected,
            "mock expectation error includes ancestry mismatch",
        )
    }

    #[test]
    fn contextual_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent("contextual parent"));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!("span");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn contextual_parent_wrong_name() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent("contextual parent"));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("another parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span named `contextual parent`,\n\
    [tests::contextual_parent_wrong_name] but got one named `another parent` instead.",
        )
    }

    #[test]
    fn contextual_parent_wrong_id() -> Result<(), TestFailure> {
        let id = expect::id();
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent(&id));

        let (subscriber, handle) = subscriber::mock()
            .new_span(&id)
            .new_span(expect::span())
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _span = tracing::info_span!("contextual parent");
            let _guard = tracing::info_span!("another parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span a span with Id `1`,\n\
    [tests::contextual_parent_wrong_id] but got one with Id `2` instead",
        )
    }

    #[test]
    fn contextual_parent_wrong_level() -> Result<(), TestFailure> {
        let parent = expect::span().at_level(Level::INFO);
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent(parent));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::debug_span!("contextual parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span at level `Level(Info)`,\n\
    [tests::contextual_parent_wrong_level] but got one at level `Level(Debug)` instead.",
        )
    }

    #[test]
    fn expect_contextual_parent_actual_contextual_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent("contextual parent"));

        let (subscriber, handle) = subscriber::mock().new_span(span).run_with_handle();

        with_default(subscriber, || {
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span, but it is actually a \
    contextual root",
        )
    }

    #[test]
    fn expect_contextual_parent_actual_explicit_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent("contextual parent"));

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::info_span!("explicit parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span, but it actually has an \
    explicit parent span",
        )
    }

    #[test]
    fn expect_contextual_parent_actual_explicit_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_contextual_parent("contextual parent"));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!(parent: None, "span");
        });

        ensure_finished_error(
            &handle,
            "to have a contextual parent span, but it is actually an \
    explicit root",
        )
    }

    #[test]
    fn contextual_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_contextual_root());

        let (subscriber, handle) = subscriber::mock().new_span(span).run_with_handle();

        with_default(subscriber, || {
            tracing::info_span!("span");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn expect_contextual_root_actual_contextual_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_contextual_root());

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to be a contextual root, but it actually has a contextual parent span",
        )
    }

    #[test]
    fn expect_contextual_root_actual_explicit_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_contextual_root());

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::info_span!("explicit parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to be a contextual root, but it actually has an explicit parent span",
        )
    }

    #[test]
    fn expect_contextual_root_actual_explicit_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_contextual_root());

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!(parent: None, "span");
        });

        ensure_finished_error(
            &handle,
            "to be a contextual root, but it is actually an explicit root",
        )
    }

    #[test]
    fn explicit_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent("explicit parent"));

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::info_span!("explicit parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn explicit_parent_wrong_name() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent("explicit parent"));

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::info_span!("another parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span named `explicit parent`,\n\
    [tests::explicit_parent_wrong_name] but got one named `another parent` instead.",
        )
    }

    #[test]
    fn explicit_parent_wrong_id() -> Result<(), TestFailure> {
        let id = expect::id();
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent(&id));

        let (subscriber, handle) = subscriber::mock()
            .new_span(&id)
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _span = tracing::info_span!("explicit parent");
            let another_span = tracing::info_span!("another parent");
            tracing::info_span!(parent: another_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span a span with Id `1`,\n\
    [tests::explicit_parent_wrong_id] but got one with Id `2` instead",
        )
    }

    #[test]
    fn explicit_parent_wrong_level() -> Result<(), TestFailure> {
        let parent = expect::span().at_level(Level::INFO);
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent(parent));

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::debug_span!("explicit parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span at level `Level(Info)`,\n\
    [tests::explicit_parent_wrong_level] but got one at level `Level(Debug)` instead.",
        )
    }

    #[test]
    fn expect_explicit_parent_actual_contextual_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent("explicit parent"));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span, but it actually has a \
    contextual parent span",
        )
    }

    #[test]
    fn expect_explicit_parent_actual_contextual_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent("explicit parent"));

        let (subscriber, handle) = subscriber::mock().new_span(span).run_with_handle();

        with_default(subscriber, || {
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span, but it is actually a \
    contextual root",
        )
    }

    #[test]
    fn expect_explicit_parent_actual_explicit_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::has_explicit_parent("explicit parent"));

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!(parent: None, "span");
        });

        ensure_finished_error(
            &handle,
            "to have an explicit parent span, but it is actually an \
    explicit root",
        )
    }

    #[test]
    fn explicit_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_explicit_root());

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!(parent: None, "span");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }

    #[test]
    fn expect_explicit_root_actual_contextual_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_explicit_root());

        let (subscriber, handle) = subscriber::mock()
            .enter(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let _guard = tracing::info_span!("contextual parent").entered();
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to be an explicit root, but it actually has a contextual parent span",
        )
    }

    #[test]
    fn expect_explicit_root_actual_contextual_root() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_explicit_root());

        let (subscriber, handle) = subscriber::mock().new_span(span).run_with_handle();

        with_default(subscriber, || {
            tracing::info_span!("span");
        });

        ensure_finished_error(
            &handle,
            "to be an explicit root, but it is actually a contextual root",
        )
    }

    #[test]
    fn expect_explicit_root_actual_explicit_parent() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_explicit_root());

        let (subscriber, handle) = subscriber::mock()
            .new_span(expect::span())
            .new_span(span)
            .run_with_handle();

        with_default(subscriber, || {
            let parent_span = tracing::info_span!("explicit parent");
            tracing::info_span!(parent: parent_span.id(), "span");
        });

        ensure_finished_error(
            &handle,
            "to be an explicit root, but it actually has an explicit parent span",
        )
    }

    #[test]
    fn explicit_and_contextual_root_is_explicit() -> Result<(), TestFailure> {
        let span = expect::span()
            .named("span")
            .with_ancestry(expect::is_explicit_root());

        let (subscriber, handle) = subscriber::mock().new_span(span).run_with_handle();

        with_default(subscriber, || {
            tracing::info_span!(parent: None, "span");
        });

        ensure_ok(handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
