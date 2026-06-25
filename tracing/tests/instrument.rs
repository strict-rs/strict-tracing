//! `Instrument` and `WithSubscriber` integration coverage.

#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
    // These tests require the thread-local scoped dispatcher, which only works when
    // we have a standard library. The behaviour being tested should be the same
    // with the standard lib disabled.

    use std::{future::Future, pin::Pin, task};

    use futures::FutureExt as _;
    use strict_test_support::{TestFailure, ensure};
    use tracing::{Instrument as _, Level, subscriber::with_default};
    use tracing_mock::*;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn span_on_drop() -> Result<(), TestFailure> {
        #[derive(Clone, Debug)]
        struct AssertSpanOnDrop;

        impl Drop for AssertSpanOnDrop {
            fn drop(&mut self) {
                tracing::info!("Drop");
            }
        }

        struct Fut {
            span_on_drop: Option<AssertSpanOnDrop>,
        }

        impl Future for Fut {
            type Output = ();

            fn poll(
                mut self: Pin<&mut Self>,
                _: &mut task::Context<'_>,
            ) -> task::Poll<Self::Output> {
                drop(self.span_on_drop.take());
                task::Poll::Ready(())
            }
        }

        let subscriber = subscriber::mock()
            .enter(expect::span().named("foo"))
            .event(
                expect::event()
                    .with_ancestry(expect::has_contextual_parent("foo"))
                    .at_level(Level::INFO),
            )
            .exit(expect::span().named("foo"))
            .enter(expect::span().named("foo"))
            .exit(expect::span().named("foo"))
            .close_span(expect::span().named("foo"))
            .enter(expect::span().named("bar"))
            .event(
                expect::event()
                    .with_ancestry(expect::has_contextual_parent("bar"))
                    .at_level(Level::INFO),
            )
            .exit(expect::span().named("bar"))
            .close_span(expect::span().named("bar"))
            .only()
            .run();

        with_default(subscriber, || -> Result<(), TestFailure> {
            // polled once
            let poll_result = Fut {
                span_on_drop: Some(AssertSpanOnDrop),
            }
            .instrument(tracing::span!(Level::TRACE, "foo"))
            .now_or_never();
            ensure(
                poll_result.is_some(),
                "instrumented future should complete when polled once",
            )?;

            // never polled
            drop(
                Fut {
                    span_on_drop: Some(AssertSpanOnDrop),
                }
                .instrument(tracing::span!(Level::TRACE, "bar")),
            );
            Ok(())
        })
    }
}
