//! Example binary for tracing workspace checks.
#![cfg(test)]

use std::any::type_name;
use std::convert::Infallible;
use std::hint::black_box;
use std::{future::Future, pin::Pin, sync::Arc};

use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok};
use tracing::field::debug;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;
use tracing_mock::{expect, subscriber};
use tracing_test::{PollN, block_on_future};

#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "async fixture remains a named instrumented function so poll and await span behavior can be asserted"
)]
async fn test_async_fn(polls: usize) -> Result<(), ()> {
    let future = PollN::new_ok(polls);
    tracing::trace!(awaiting = true);
    future.await
}

// Reproduces a compile error when returning an `impl Trait` from an
// instrumented async fn (see https://github.com/tokio-rs/tracing/issues/1615)
#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "async impl Trait regression fixture must remain a named instrumented function item"
)]
async fn test_ret_impl_trait(limit: i32) -> Result<impl Iterator<Item = i32>, ()> {
    Ok((0..10).filter(move |value| *value < limit))
}

// Reproduces a compile error when returning an `impl Trait` from an
// instrumented async fn (see https://github.com/tokio-rs/tracing/issues/1615)
#[instrument(err)]
#[allow(
    clippy::single_call_fn,
    reason = "async err impl Trait regression fixture must remain a named instrumented function item"
)]
async fn test_ret_impl_trait_err(limit: i32) -> Result<impl Iterator<Item = i32>, &'static str> {
    Ok((0..10).filter(move |value| *value < limit))
}

#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "empty async fixture remains a named instrumented function so empty body expansion is asserted"
)]
async fn test_async_fn_empty() {}

// Reproduces a compile error when an instrumented function body contains inner
// attributes (https://github.com/tokio-rs/tracing/issues/2294).
#[deny(unused_variables)]
#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "async inner-attribute regression fixture must remain a named instrumented function item"
)]
async fn repro_async_2294() {
    let observed_value = 42;
    let _observed = black_box(observed_value);
}

// Reproduces https://github.com/tokio-rs/tracing/issues/1613
#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "suspicious-else regression fixture must remain a named instrumented async function item"
)]
// LOAD-BEARING `#[rustfmt::skip]`! This is necessary to reproduce the bug;
// with the rustfmt-generated formatting, the lint will not be triggered!
#[rustfmt::skip]
#[deny(clippy::suspicious_else_formatting)]
async fn repro_1613(var: bool) {
    let _rendered = black_box(if var { "true" } else { "false" });
}

// Reproduces https://github.com/tokio-rs/tracing/issues/1613
// and https://github.com/rust-lang/rust-clippy/issues/7760
#[instrument]
#[allow(
    clippy::single_call_fn,
    reason = "suspicious-else comment regression fixture must remain a named instrumented async function item"
)]
#[deny(clippy::suspicious_else_formatting)]
async fn repro_1613_2() {
    // hello world
    // else
}

// Reproduces https://github.com/tokio-rs/tracing/issues/1831
#[instrument]
#[deny(unused_braces)]
fn repro_1831() -> Pin<Box<dyn Future<Output = ()>>> {
    Box::pin(async move {})
}

// This replicates the pattern used to implement async trait methods on nightly using the
// `type_alias_impl_trait` feature
#[instrument(ret, err)]
#[deny(unused_braces)]
fn repro_1831_2() -> impl Future<Output = Result<(), Infallible>> {
    async { Ok(()) }
}

#[test]
fn async_compile_repros_run() -> Result<(), TestFailure> {
    block_on_future(async {
        let Ok(values) = test_ret_impl_trait(3).await else {
            return ensure(false, "instrumented async impl Trait result should be Ok");
        };
        ensure_eq(
            &values.count(),
            &3_usize,
            "instrumented async impl Trait iterator should retain values",
        )?;

        let Ok(err_values) = test_ret_impl_trait_err(4).await else {
            return ensure(
                false,
                "instrumented async err impl Trait result should be Ok",
            );
        };
        ensure_eq(
            &err_values.count(),
            &4_usize,
            "instrumented async err impl Trait iterator should retain values",
        )?;

        test_async_fn_empty().await;
        repro_async_2294().await;
        repro_1613(true).await;
        repro_1613_2().await;
        repro_1831().await;
        match repro_1831_2().await {
            Ok(()) => {}
            Err(error) => match error {},
        }

        Ok(())
    })
}

#[test]
fn async_fn_only_enters_for_polls() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
        .new_span(expect::span().named("test_async_fn"))
        .enter(expect::span().named("test_async_fn"))
        .event(expect::event().with_fields(expect::field("awaiting").with_value(&true)))
        .exit(expect::span().named("test_async_fn"))
        .enter(expect::span().named("test_async_fn"))
        .exit(expect::span().named("test_async_fn"))
        .enter(expect::span().named("test_async_fn"))
        .exit(expect::span().named("test_async_fn"))
        .close_span(expect::span().named("test_async_fn"))
        .only()
        .run_with_handle();
    with_default(subscriber, || {
        let Ok(()) = block_on_future(async { test_async_fn(2).await }) else {
            return ensure(false, "instrumented async function should complete");
        };
        Ok(())
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn async_fn_nested() -> Result<(), TestFailure> {
    #[instrument]
    async fn test_async_fns_nested() {
        test_async_fns_nested_other().await
    }

    #[instrument]
    async fn test_async_fns_nested_other() {
        tracing::trace!(nested = true);
    }

    let span = expect::span().named("test_async_fns_nested");
    let span2 = expect::span().named("test_async_fns_nested_other");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .new_span(span2.clone())
        .enter(span2.clone())
        .event(expect::event().with_fields(expect::field("nested").with_value(&true)))
        .exit(span2.clone())
        .enter(span2.clone())
        .exit(span2.clone())
        .close_span(span2)
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async { test_async_fns_nested().await });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn async_fn_with_async_trait() -> Result<(), TestFailure> {
    use async_trait::async_trait;

    // test the correctness of the metadata obtained by #[instrument]
    // (function name, functions parameters) when async-trait is used
    #[async_trait]
    pub(crate) trait TestA {
        async fn foo(&mut self, value: usize);
    }

    // test nesting of async fns with aync-trait
    #[async_trait]
    pub(crate) trait TestB {
        async fn bar(&self);
    }

    // test skip(self) with async-await
    #[async_trait]
    pub(crate) trait TestC {
        async fn baz(&self);
    }

    #[derive(Debug)]
    struct TestImpl(usize);

    #[async_trait]
    impl TestA for TestImpl {
        #[instrument(skip(value), fields(v = value))]
        async fn foo(&mut self, value: usize) {
            self.baz().await;
            self.0 = value;
            self.bar().await;
        }
    }

    #[async_trait]
    impl TestB for TestImpl {
        #[instrument]
        async fn bar(&self) {
            tracing::trace!(val = self.0);
        }
    }

    #[async_trait]
    impl TestC for TestImpl {
        #[instrument(skip(self))]
        async fn baz(&self) {
            tracing::trace!(val = self.0);
        }
    }

    let span = expect::span().named("foo");
    let span2 = expect::span().named("bar");
    let span3 = expect::span().named("baz");
    let (subscriber, handle) = subscriber::mock()
        .new_span(
            span.clone()
                .with_fields(expect::field("self"))
                .with_fields(expect::field("v")),
        )
        .enter(span.clone())
        .new_span(span3.clone())
        .enter(span3.clone())
        .event(expect::event().with_fields(expect::field("val").with_value(&2_u64)))
        .exit(span3.clone())
        .enter(span3.clone())
        .exit(span3.clone())
        .close_span(span3)
        .new_span(span2.clone().with_fields(expect::field("self")))
        .enter(span2.clone())
        .event(expect::event().with_fields(expect::field("val").with_value(&5_u64)))
        .exit(span2.clone())
        .enter(span2.clone())
        .exit(span2.clone())
        .close_span(span2)
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        let mut test = TestImpl(2);
        block_on_future(async { test.foo(5).await });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn async_fn_with_async_trait_and_fields_expressions() -> Result<(), TestFailure> {
    use async_trait::async_trait;

    #[async_trait]
    pub(crate) trait Test {
        async fn call(&mut self, value: usize);
    }

    #[derive(Clone, Debug)]
    struct TestImpl;

    impl TestImpl {
        fn foo(&self) -> usize {
            let observed_self = format!("{self:?}");
            drop(observed_self);
            42
        }
    }

    #[async_trait]
    impl Test for TestImpl {
        // check that self is correctly handled, even when using async_trait
        #[instrument(fields(val=self.foo(), val2=Self::clone(self).foo(), test=%_v+5))]
        async fn call(&mut self, _v: usize) {}
    }

    let span = expect::span().named("call");
    let (subscriber, handle) = subscriber::mock()
        .new_span(
            span.clone().with_fields(
                expect::field("_v")
                    .with_value(&5_usize)
                    .and(expect::field("test").with_value(&debug(10)))
                    .and(expect::field("val").with_value(&42_u64))
                    .and(expect::field("val2").with_value(&42_u64)),
            ),
        )
        .enter(span.clone())
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async { TestImpl.call(5).await });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn async_fn_with_async_trait_and_fields_expressions_with_generic_parameter()
-> Result<(), TestFailure> {
    use async_trait::async_trait;

    #[async_trait]
    pub(crate) trait Test {
        async fn call();
        async fn call_with_self(&self);
        async fn call_with_mut_self(&mut self);
    }

    #[derive(Clone, Debug)]
    struct TestImpl;

    // we also test sync functions that return futures, as they should be handled just like
    // async-trait (>= 0.1.44) functions
    impl TestImpl {
        #[instrument(fields(Self=type_name::<Self>()))]
        fn sync_fun(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            let val = self.clone();
            Box::pin(async move {
                let observed_self = format!("{val:?}");
                drop(observed_self);
            })
        }
    }

    #[async_trait]
    impl Test for TestImpl {
        // instrumenting this is currently not possible, see https://github.com/tokio-rs/tracing/issues/864#issuecomment-667508801
        //#[instrument(fields(Self=type_name::<Self>()))]
        async fn call() {}

        #[instrument(fields(Self=type_name::<Self>()))]
        async fn call_with_self(&self) {
            self.sync_fun().await;
        }

        #[instrument(fields(Self=type_name::<Self>()))]
        async fn call_with_mut_self(&mut self) {
            let _observed_self: &mut Self = self;
        }
    }

    //let span = span::mock().named("call");
    let span2 = expect::span().named("call_with_self");
    let span3 = expect::span().named("call_with_mut_self");
    let span4 = expect::span().named("sync_fun");
    let (subscriber, handle) = subscriber::mock()
        /*.new_span(span.clone()
            .with_fields(
                expect::field("Self").with_value(&"TestImpler")))
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)*/
        .new_span(
            span2
                .clone()
                .with_fields(expect::field("Self").with_value(&type_name::<TestImpl>())),
        )
        .enter(span2.clone())
        .new_span(
            span4
                .clone()
                .with_fields(expect::field("Self").with_value(&type_name::<TestImpl>())),
        )
        .enter(span4.clone())
        .exit(span4.clone())
        .enter(span4.clone())
        .exit(span4)
        .exit(span2.clone())
        .enter(span2.clone())
        .exit(span2.clone())
        .close_span(span2)
        .new_span(
            span3
                .clone()
                .with_fields(expect::field("Self").with_value(&type_name::<TestImpl>())),
        )
        .enter(span3.clone())
        .exit(span3.clone())
        .enter(span3.clone())
        .exit(span3.clone())
        .close_span(span3)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async {
            TestImpl::call().await;
            TestImpl.call_with_self().await;
            TestImpl.call_with_mut_self().await;
        });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn out_of_scope_fields() -> Result<(), TestFailure> {
    // Reproduces tokio-rs/tracing#1296

    struct Thing {
        metrics: Arc<()>,
    }

    impl Thing {
        #[instrument(skip(self, _req), fields(app_id))]
        fn call(&mut self, _req: ()) -> Pin<Box<dyn Future<Output = Arc<()>> + Send + Sync>> {
            // ...
            let metrics = Arc::clone(&self.metrics);
            // ...
            Box::pin(async move {
                // ...
                metrics // cannot find value `metrics` in this scope
            })
        }
    }

    let span = expect::span().named("call");
    let (subscriber, handle) = subscriber::mock()
        .new_span(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async {
            let mut my_thing = Thing {
                metrics: Arc::new(()),
            };
            let _metrics = my_thing.call(()).await;
        });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn manual_impl_future() -> Result<(), TestFailure> {
    #[instrument]
    fn manual_impl_future() -> impl Future<Output = ()> {
        async {
            tracing::trace!(poll = true);
        }
    }

    let span = expect::span().named("manual_impl_future");
    let poll_event = || expect::event().with_fields(expect::field("poll").with_value(&true));

    let (subscriber, handle) = subscriber::mock()
        // await manual_impl_future
        .new_span(span.clone())
        .enter(span.clone())
        .event(poll_event())
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async {
            manual_impl_future().await;
        });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}

#[test]
fn manual_box_pin() -> Result<(), TestFailure> {
    #[instrument]
    fn manual_box_pin() -> Pin<Box<dyn Future<Output = ()>>> {
        Box::pin(async {
            tracing::trace!(poll = true);
        })
    }

    let span = expect::span().named("manual_box_pin");
    let poll_event = || expect::event().with_fields(expect::field("poll").with_value(&true));

    let (subscriber, handle) = subscriber::mock()
        // await manual_box_pin
        .new_span(span.clone())
        .enter(span.clone())
        .event(poll_event())
        .exit(span.clone())
        .enter(span.clone())
        .exit(span.clone())
        .close_span(span)
        .only()
        .run_with_handle();

    with_default(subscriber, || {
        block_on_future(async {
            manual_box_pin().await;
        });
    });

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
