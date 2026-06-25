use tracing_core::{
    Dispatch, Event, Interest, LevelFilter, Subscriber, SubscriberResult, metadata::Metadata, span,
};

use crate::{
    filter,
    layer::{Context, Layer},
    registry::LookupSpan,
};
#[cfg(all(feature = "registry", feature = "std"))]
use crate::{
    filter::FilterId,
    registry::{CloseSpan as _, Registry},
};
use core::{
    any::{Any, TypeId},
    cmp, fmt,
    marker::PhantomData,
};

/// A [`Subscriber`] composed of a `Subscriber` wrapped by one or more
/// [`Layer`]s.
///
/// [`Layer`]: crate::Layer
/// [`Subscriber`]: tracing_core::Subscriber
#[derive(Clone)]
pub struct Layered<L, I, S = I> {
    /// The layer.
    layer: L,

    /// The inner value that `self.layer` was layered onto.
    ///
    /// If this is also a `Layer`, then this `Layered` will implement `Layer`.
    /// If this is a `Subscriber`, then this `Layered` will implement
    /// `Subscriber` instead.
    inner: I,

    /// Per-layer filter state used to combine `Interest`s and max level hints.
    filters: LayeredFilterState,
    /// Tracks the subscriber type parameter when this value is a nested layer.
    _s: PhantomData<fn(S)>,
}

/// Compact per-layer filter flags for a [`Layered`] value.
#[derive(Clone, Copy, Debug)]
struct LayeredFilterState {
    /// Packed flag bits.
    flags: u8,
}

impl LayeredFilterState {
    /// Outer layer has a per-layer filter.
    const HAS_LAYER_FILTER: u8 = 0b001;
    /// Inner stack has per-layer filters.
    const INNER_HAS_LAYER_FILTER: u8 = 0b010;
    /// Inner subscriber is the registry.
    const INNER_IS_REGISTRY: u8 = 0b100;

    /// Returns `true` when the outer layer has a per-layer filter.
    #[must_use]
    const fn has_layer_filter(self) -> bool {
        self.flags & Self::HAS_LAYER_FILTER != 0
    }

    /// Returns `true` when the inner stack has per-layer filters.
    #[must_use]
    const fn inner_has_layer_filter(self) -> bool {
        self.flags & Self::INNER_HAS_LAYER_FILTER != 0
    }

    /// Returns `true` when the inner subscriber is the registry.
    #[must_use]
    const fn inner_is_registry(self) -> bool {
        self.flags & Self::INNER_IS_REGISTRY != 0
    }
}

// === impl Layered ===

impl<L, I> Layered<L, I>
where
    L: Layer<I>,
    I: Subscriber,
{
    /// Returns `true` if this [`Subscriber`] is the same type as `T`.
    #[must_use]
    pub fn is<T: Any>(&self) -> bool {
        self.downcast_ref::<T>().is_some()
    }

    /// Returns some reference to this [`Subscriber`] value if it is of type `T`,
    /// or `None` if it isn't.
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.downcast_ref_by_id(TypeId::of::<T>())?
            .downcast_ref::<T>()
    }
}

impl<L, I> Subscriber for Layered<L, I>
where
    L: Layer<I>,
    I: Subscriber,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) -> SubscriberResult {
        self.inner.on_register_dispatch(subscriber)?;
        self.layer.on_register_dispatch(subscriber)
    }

    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        self.pick_interest(self.layer.register_callsite(metadata)?, || {
            self.inner.register_callsite(metadata)
        })
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        if self.layer.enabled(metadata, self.ctx())? {
            // if the outer layer enables the callsite metadata, ask the subscriber.
            self.inner.enabled(metadata)
        } else {
            // otherwise, the callsite is disabled by the layer

            // If per-layer filters are in use, and we are short-circuiting
            // (rather than calling into the inner type), clear the current
            // per-layer filter `enabled` state.
            #[cfg(feature = "registry")]
            filter::FilterState::clear_enabled();

            Ok(false)
        }
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.pick_level_hint(
            self.layer.max_level_hint().ok().flatten(),
            self.inner.max_level_hint(),
            self.inner
                .downcast_ref_by_id(TypeId::of::<super::NoneLayerMarker>())
                .is_some(),
        )
    }

    fn new_span(&self, span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
        let id = self.inner.new_span(span)?;
        self.layer.on_new_span(span, id, self.ctx())?;
        Ok(id)
    }

    fn record(&self, span: span::Id, values: &span::Record<'_>) -> SubscriberResult {
        self.inner.record(span, values)?;
        self.layer.on_record(span, values, self.ctx())
    }

    fn record_follows_from(&self, span: span::Id, follows: span::Id) -> SubscriberResult {
        self.inner.record_follows_from(span, follows)?;
        self.layer.on_follows_from(span, follows, self.ctx())
    }

    fn event_enabled(&self, event: &Event<'_>) -> SubscriberResult<bool> {
        if self.layer.event_enabled(event, self.ctx())? {
            // if the outer layer enables the event, ask the inner subscriber.
            self.inner.event_enabled(event)
        } else {
            // otherwise, the event is disabled by this layer
            Ok(false)
        }
    }

    fn event(&self, event: &Event<'_>) -> SubscriberResult {
        self.inner.event(event)?;
        self.layer.on_event(event, self.ctx())
    }

    fn enter(&self, span: span::Id) -> SubscriberResult {
        self.inner.enter(span)?;
        self.layer.on_enter(span, self.ctx())
    }

    fn exit(&self, span: span::Id) -> SubscriberResult {
        self.inner.exit(span)?;
        self.layer.on_exit(span, self.ctx())
    }

    fn clone_span(&self, old: span::Id) -> SubscriberResult<span::Id> {
        let new = self.inner.clone_span(old)?;
        if new != old {
            self.layer.on_id_change(old, new, self.ctx())?;
        }
        Ok(new)
    }

    fn try_close(&self, id: span::Id) -> SubscriberResult<bool> {
        #[cfg(all(feature = "registry", feature = "std"))]
        let subscriber: &dyn Subscriber = &self.inner;
        #[cfg(all(feature = "registry", feature = "std"))]
        let mut close_handle = subscriber
            .downcast_ref::<Registry>()
            .map(|registry| registry.start_close(id));
        if self.inner.try_close(id)? {
            // If we have a registry's close guard, indicate that the span is
            // closing.
            #[cfg(all(feature = "registry", feature = "std"))]
            {
                if let Some(handle) = close_handle.as_mut() {
                    handle.set_closing();
                }
            }

            self.layer.on_close(id, self.ctx())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[inline]
    fn current_span(&self) -> SubscriberResult<span::Current> {
        self.inner.current_span()
    }

    #[doc(hidden)]
    fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
        // Unlike the implementation of `Layer` for `Layered`, we don't have to
        // handle the "magic PLF downcast marker" here. If a `Layered`
        // implements `Subscriber`, we already know that the `inner` branch is
        // going to contain something that doesn't have per-layer filters (the
        // actual root `Subscriber`). Thus, a `Layered` that implements
        // `Subscriber` will always be propagating the root subscriber's
        // `Interest`/level hint, even if it includes a `Layer` that has
        // per-layer filters, because it will only ever contain layers where
        // _one_ child has per-layer filters.
        //
        // The complex per-layer filter detection logic is only relevant to
        // *trees* of layers, which involve the `Layer` implementation for
        // `Layered`, not *lists* of layers, where every `Layered` implements
        // `Subscriber`. Of course, a linked list can be thought of as a
        // degenerate tree...but luckily, we are able to make a type-level
        // distinction between individual `Layered`s that are definitely
        // list-shaped (their inner child implements `Subscriber`), and
        // `Layered`s that might be tree-shaped (the inner child is also a
        // `Layer`).

        if id == TypeId::of::<Self>() {
            return Some(self);
        }

        self.layer
            .downcast_ref_by_id(id)
            .or_else(|| self.inner.downcast_ref_by_id(id))
    }
}

impl<S, A, B> Layer<S> for Layered<A, B, S>
where
    A: Layer<S>,
    B: Layer<S>,
    S: Subscriber,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) -> SubscriberResult {
        self.layer.on_register_dispatch(subscriber)?;
        self.inner.on_register_dispatch(subscriber)
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        self.layer.on_layer(subscriber);
        self.inner.on_layer(subscriber);
    }

    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        self.pick_interest(self.layer.register_callsite(metadata)?, || {
            self.inner.register_callsite(metadata)
        })
    }

    fn enabled(&self, metadata: &Metadata<'_>, ctx: Context<'_, S>) -> SubscriberResult<bool> {
        if self.layer.enabled(metadata, ctx.clone())? {
            // if the outer subscriber enables the callsite metadata, ask the inner layer.
            self.inner.enabled(metadata, ctx)
        } else {
            // otherwise, the callsite is disabled by this layer
            Ok(false)
        }
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(self.pick_level_hint(
            self.layer.max_level_hint()?,
            self.inner.max_level_hint()?,
            super::layer_is_none(&self.inner),
        ))
    }

    #[inline]
    fn on_new_span(
        &self,
        attrs: &span::Attributes<'_>,
        id: span::Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult {
        self.inner.on_new_span(attrs, id, ctx.clone())?;
        self.layer.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(
        &self,
        span: span::Id,
        values: &span::Record<'_>,
        ctx: Context<'_, S>,
    ) -> SubscriberResult {
        self.inner.on_record(span, values, ctx.clone())?;
        self.layer.on_record(span, values, ctx)
    }

    #[inline]
    fn on_follows_from(
        &self,
        span: span::Id,
        follows: span::Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult {
        self.inner.on_follows_from(span, follows, ctx.clone())?;
        self.layer.on_follows_from(span, follows, ctx)
    }

    #[inline]
    fn event_enabled(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult<bool> {
        if self.layer.event_enabled(event, ctx.clone())? {
            // if the outer layer enables the event, ask the inner subscriber.
            self.inner.event_enabled(event, ctx)
        } else {
            // otherwise, the event is disabled by this layer
            Ok(false)
        }
    }

    #[inline]
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_event(event, ctx.clone())?;
        self.layer.on_event(event, ctx)
    }

    #[inline]
    fn on_enter(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_enter(id, ctx.clone())?;
        self.layer.on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_exit(id, ctx.clone())?;
        self.layer.on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_close(id, ctx.clone())?;
        self.layer.on_close(id, ctx)
    }

    #[inline]
    fn on_id_change(&self, old: span::Id, new: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
        self.inner.on_id_change(old, new, ctx.clone())?;
        self.layer.on_id_change(old, new, ctx)
    }

    #[doc(hidden)]
    fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
        match id {
            downcast_id if downcast_id == TypeId::of::<Self>() => Some(self),

            // Oh, we're looking for per-layer filters!
            //
            // This should only happen if we are inside of another `Layered`,
            // and it's trying to determine how it should combine `Interest`s
            // and max level hints.
            //
            // In that case, this `Layered` should be considered to be
            // "per-layer filtered" if *both* the outer layer and the inner
            // layer/subscriber have per-layer filters. Otherwise, this `Layered
            // should *not* be considered per-layer filtered (even if one or the
            // other has per layer filters). If only one `Layer` is per-layer
            // filtered, *this* `Layered` will handle aggregating the `Interest`
            // and level hints on behalf of its children, returning the
            // aggregate (which is the value from the &non-per-layer-filtered*
            // child).
            //
            // Yes, this rule *is* slightly counter-intuitive, but it's
            // necessary due to a weird edge case that can occur when two
            // `Layered`s where one side is per-layer filtered and the other
            // isn't are `Layered` together to form a tree. If we didn't have
            // this rule, we would actually end up *ignoring* `Interest`s from
            // the non-per-layer-filtered layers, since both branches would
            // claim to have PLF.
            //
            // If you don't understand this...that's fine, just don't mess with
            // it. :)
            downcast_id if filter::is_plf_downcast_marker(downcast_id) => {
                let _: &dyn Any = self.layer.downcast_ref_by_id(downcast_id)?;
                self.inner.downcast_ref_by_id(downcast_id)
            }

            // Otherwise, try to downcast both branches normally...
            _ => self
                .layer
                .downcast_ref_by_id(id)
                .or_else(|| self.inner.downcast_ref_by_id(id)),
        }
    }
}

impl<'a, L, I> LookupSpan<'a> for Layered<L, I>
where
    I: Subscriber + LookupSpan<'a>,
{
    type Data = I::Data;

    fn span_data(&'a self, id: span::Id) -> Option<Self::Data> {
        self.inner.span_data(id)
    }

    #[cfg(all(feature = "registry", feature = "std"))]
    fn register_filter(&mut self) -> FilterId {
        self.inner.register_filter()
    }
}

impl<L, I> Layered<L, I>
where
    I: Subscriber,
{
    /// Returns a context for the inner subscriber.
    const fn ctx(&self) -> Context<'_, I> {
        Context::new(&self.inner)
    }
}

impl<A, B, S> Layered<A, B, S>
where
    A: Layer<S>,
    S: Subscriber,
{
    /// Returns a new layered value.
    pub(super) fn new(layer: A, inner: B, inner_has_layer_filter: bool) -> Self {
        #[cfg(all(feature = "registry", feature = "std"))]
        let inner_is_registry = TypeId::of::<S>() == TypeId::of::<Registry>();

        #[cfg(not(all(feature = "registry", feature = "std")))]
        let inner_is_registry = false;

        let inner_filter = inner_has_layer_filter || inner_is_registry;
        let has_layer_filter = filter::layer_has_plf(&layer);
        let mut filter_flags = 0;
        if has_layer_filter {
            filter_flags |= LayeredFilterState::HAS_LAYER_FILTER;
        }
        if inner_filter {
            filter_flags |= LayeredFilterState::INNER_HAS_LAYER_FILTER;
        }
        if inner_is_registry {
            filter_flags |= LayeredFilterState::INNER_IS_REGISTRY;
        }
        let filters = LayeredFilterState {
            flags: filter_flags,
        };
        Self {
            layer,
            inner,
            filters,
            _s: PhantomData,
        }
    }

    /// Combines outer and inner callsite interest.
    fn pick_interest(
        &self,
        outer: Interest,
        inner_interest: impl FnOnce() -> SubscriberResult<Interest>,
    ) -> SubscriberResult<Interest> {
        if self.filters.has_layer_filter() {
            return inner_interest();
        }

        // If the outer layer has disabled the callsite, return now so that
        // the inner layer/subscriber doesn't get its hopes up.
        if outer.is_never() {
            // If per-layer filters are in use, and we are short-circuiting
            // (rather than calling into the inner type), clear the current
            // per-layer filter interest state.
            #[cfg(feature = "registry")]
            let _: Option<Interest> = filter::FilterState::take_interest();

            return Ok(outer);
        }

        // The `inner` closure will call `inner.register_callsite()`. We do this
        // before the `if` statement to  ensure that the inner subscriber is
        // informed that the callsite exists regardless of the outer layer's
        // filtering decision.
        let inner = inner_interest()?;
        if outer.is_sometimes() {
            // if this interest is "sometimes", return "sometimes" to ensure that
            // filters are reevaluated.
            return Ok(outer);
        }

        // If there is a per-layer filter in the `inner` stack, and it returns
        // `never`, change the interest to `sometimes`, because the `outer`
        // layer didn't return `never`. This means that _some_ layer still wants
        // to see that callsite, even though the inner stack's per-layer filter
        // didn't want it. Therefore, returning `sometimes` will ensure
        // `enabled` is called so that the per-layer filter can skip that
        // span/event, while the `outer` layer still gets to see it.
        if inner.is_never() && self.filters.inner_has_layer_filter() {
            return Ok(Interest::sometimes());
        }

        // otherwise, allow the inner subscriber or subscriber to weigh in.
        Ok(inner)
    }

    /// Combines outer and inner max-level hints.
    fn pick_level_hint(
        &self,
        outer_hint: Option<LevelFilter>,
        inner_hint: Option<LevelFilter>,
        inner_is_none: bool,
    ) -> Option<LevelFilter> {
        if self.filters.inner_is_registry() {
            return outer_hint;
        }

        if self.filters.has_layer_filter() && self.filters.inner_has_layer_filter() {
            return Some(cmp::max(outer_hint?, inner_hint?));
        }

        if self.filters.has_layer_filter() && inner_hint.is_none() {
            return None;
        }

        if self.filters.inner_has_layer_filter() && outer_hint.is_none() {
            return None;
        }

        // If the layer is `Option::None`, then we
        // want to short-circuit the layer underneath, if it
        // returns `None`, to override the `None` layer returning
        // `Some(OFF)`, which should ONLY apply when there are
        // no other layers that return `None`. Note this
        // `None` does not == `Some(TRACE)`, it means
        // something more like: "whatever all the other
        // layers agree on, default to `TRACE` if none
        // have an opinion". We also choose do this AFTER
        // we check for per-layer filters, which
        // have their own logic.
        //
        // Also note that this does come at some perf cost, but
        // this function is only called on initialization and
        // subscriber reloading.
        if super::layer_is_none(&self.layer) {
            return cmp::max(outer_hint, Some(inner_hint?));
        }

        // Similarly, if the layer on the inside is `None` and it returned an
        // `Off` hint, we want to override that with the outer hint.
        if inner_is_none && inner_hint == Some(LevelFilter::OFF) {
            return outer_hint;
        }

        cmp::max(outer_hint, inner_hint)
    }
}

impl<A, B, S> fmt::Debug for Layered<A, B, S>
where
    A: fmt::Debug,
    B: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(all(feature = "registry", feature = "std"))]
        let alt = formatter.alternate();
        let mut debug = formatter.debug_struct("Layered");
        // These additional fields are more verbose and usually only necessary
        // for internal debugging purposes, so only print them if alternate mode
        // is enabled.

        #[cfg(all(feature = "registry", feature = "std"))]
        {
            if alt {
                let _debug = debug
                    .field("inner_is_registry", &self.filters.inner_is_registry())
                    .field("has_layer_filter", &self.filters.has_layer_filter())
                    .field(
                        "inner_has_layer_filter",
                        &self.filters.inner_has_layer_filter(),
                    );
            }
        }

        debug
            .field("layer", &self.layer)
            .field("inner", &self.inner)
            .finish()
    }
}
