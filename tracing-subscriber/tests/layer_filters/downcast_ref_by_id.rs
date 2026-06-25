use std::any::{Any, TypeId};
use strict_test_support::{TestFailure, ensure};
use tracing::{Dispatch, Level, Subscriber};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{Layer, filter::Targets, registry::LookupSpan};

#[test]
fn downcast_ref_to_inner_layer_and_filter() -> Result<(), TestFailure> {
    // Test that a filtered layer gives downcast_ref access to
    // both the layer and the filter.

    struct WrappedLayer;

    impl<S> Layer<S> for WrappedLayer where S: Subscriber + for<'lookup> LookupSpan<'lookup> {}

    let layer = WrappedLayer;
    let filter = Targets::new().with_default(Level::INFO);
    let registry = tracing_subscriber::registry().with(layer.with_filter(filter));
    let dispatch = Dispatch::new(registry);

    // The filter is available
    ensure(
        dispatch.downcast_ref::<Targets>().is_some(),
        "filtered layer exposes the filter by type",
    )?;
    // The wrapped layer is available
    ensure(
        dispatch.downcast_ref::<WrappedLayer>().is_some(),
        "filtered layer exposes the wrapped layer by type",
    )
}

#[test]
fn forward_downcast_ref_by_id_to_layer() -> Result<(), TestFailure> {
    // Test that a filtered layer still gives its wrapped layer a chance to
    // return a custom struct from downcast_ref_by_id.
    // https://github.com/tokio-rs/tracing/issues/1618

    struct WrappedLayer {
        with_context: WithContext,
    }

    struct WithContext;

    impl<S> Layer<S> for WrappedLayer
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn downcast_ref_by_id(&self, type_id: TypeId) -> Option<&dyn Any> {
            match type_id {
                current if current == TypeId::of::<Self>() => Some(self),
                current if current == TypeId::of::<WithContext>() => Some(&self.with_context),
                _ => None,
            }
        }
    }

    let layer = WrappedLayer {
        with_context: WithContext,
    };
    let filter = Targets::new().with_default(Level::INFO);
    let registry = tracing_subscriber::registry().with(layer.with_filter(filter));
    let dispatch = Dispatch::new(registry);

    // Types from a custom implementation of `downcast_ref_by_id` are available
    ensure(
        dispatch.downcast_ref::<WithContext>().is_some(),
        "custom downcast_ref_by_id types are forwarded",
    )
}
