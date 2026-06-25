//! Filter combinators
use crate::layer::{Context, Filter};
use std::{cmp, fmt, marker::PhantomData};
use tracing_core::{
    span::{Attributes, Id, Record},
    subscriber::{Interest, SubscriberResult},
    LevelFilter, Metadata,
};

/// Combines two [`Filter`]s so that spans and events are enabled if and only if
/// *both* filters return `true`.
///
/// This type is typically returned by the [`FilterExt::and`] method. See that
/// method's documentation for details.
///
/// [`Filter`]: crate::layer::Filter
/// [`FilterExt::and`]: crate::filter::FilterExt::and
pub struct And<A, B, S> {
    /// The first filter in the conjunction.
    left: A,
    /// The second filter in the conjunction.
    right: B,
    /// Connects the combinator to the subscriber type observed by its filters.
    _subscriber: PhantomData<fn(S)>,
}

/// Combines two [`Filter`]s so that spans and events are enabled if *either* filter
/// returns `true`.
///
/// This type is typically returned by the [`FilterExt::or`] method. See that
/// method's documentation for details.
///
/// [`Filter`]: crate::layer::Filter
/// [`FilterExt::or`]: crate::filter::FilterExt::or
pub struct Or<A, B, S> {
    /// The first filter in the disjunction.
    left: A,
    /// The second filter in the disjunction.
    right: B,
    /// Connects the combinator to the subscriber type observed by its filters.
    _subscriber: PhantomData<fn(S)>,
}

/// Inverts the result of a [`Filter`].
///
/// If the wrapped filter would enable a span or event, it will be disabled. If
/// it would disable a span or event, that span or event will be enabled.
///
/// This type is typically returned by the [`FilterExt::not`] method. See that
/// method's documentation for details.
///
/// [`Filter`]: crate::layer::Filter
/// [`FilterExt::not`]: crate::filter::FilterExt::not
pub struct Not<A, S> {
    /// The filter whose decision is inverted.
    inner: A,
    /// Connects the combinator to the subscriber type observed by its filter.
    _subscriber: PhantomData<fn(S)>,
}

// === impl And ===

impl<A, B, S> And<A, B, S>
where
    A: Filter<S>,
    B: Filter<S>,
{
    /// Combines two [`Filter`]s so that spans and events are enabled if and only if
    /// *both* filters return `true`.
    ///
    /// # Examples
    ///
    /// Enabling spans or events if they have both a particular target *and* are
    /// above a certain level:
    ///
    /// ```ignore
    /// use tracing_subscriber::{
    ///     filter::{filter_fn, LevelFilter, combinator::And},
    ///     prelude::*,
    /// };
    ///
    /// // Enables spans and events with targets starting with `interesting_target`:
    /// let target_filter = filter_fn(|meta| {
    ///     meta.target().starts_with("interesting_target")
    /// });
    ///
    /// // Enables spans and events with levels `INFO` and below:
    /// let level_filter = LevelFilter::INFO;
    ///
    /// // Combine the two filters together so that a span or event is only enabled
    /// // if *both* filters would enable it:
    /// let filter = And::new(level_filter, target_filter);
    ///
    /// tracing_subscriber::registry()
    ///     .with(tracing_subscriber::fmt::layer().with_filter(filter))
    ///     .try_init()?;
    ///
    /// // This event will *not* be enabled:
    /// tracing::info!("an event with an uninteresting target");
    ///
    /// // This event *will* be enabled:
    /// tracing::info!(target: "interesting_target", "a very interesting event");
    ///
    /// // This event will *not* be enabled:
    /// tracing::debug!(target: "interesting_target", "interesting debug event...");
    /// ```
    ///
    /// [`Filter`]: crate::layer::Filter
    #[allow(
        clippy::single_call_fn,
        reason = "public combinator constructor is part of the layer-filter API"
    )]
    pub const fn new(left: A, right: B) -> Self {
        Self {
            left,
            right,
            _subscriber: PhantomData,
        }
    }
}

impl<A, B, S> Filter<S> for And<A, B, S>
where
    A: Filter<S>,
    B: Filter<S>,
{
    #[inline]
    fn enabled(&self, meta: &Metadata<'_>, cx: &Context<'_, S>) -> SubscriberResult<bool> {
        Ok(self.left.enabled(meta, cx)? && self.right.enabled(meta, cx)?)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        let left_interest = self.left.callsite_enabled(meta)?;
        if left_interest.is_never() {
            return Ok(left_interest);
        }

        let right_interest = self.right.callsite_enabled(meta)?;

        if !right_interest.is_always() {
            return Ok(right_interest);
        }

        Ok(left_interest)
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        // If either hint is `None`, return `None`. Otherwise, return the most restrictive.
        Ok(cmp::min(
            self.left.max_level_hint()?,
            self.right.max_level_hint()?,
        ))
    }

    #[inline]
    fn event_enabled(
        &self,
        event: &tracing_core::Event<'_>,
        cx: &Context<'_, S>,
    ) -> SubscriberResult<bool> {
        Ok(self.left.event_enabled(event, cx)? && self.right.event_enabled(event, cx)?)
    }

    #[inline]
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        id: Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult<()> {
        self.left.on_new_span(attrs, id, ctx.clone())?;
        self.right.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, id: Id, values: &Record<'_>, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_record(id, values, ctx.clone())?;
        self.right.on_record(id, values, ctx)
    }

    #[inline]
    fn on_enter(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_enter(id, ctx.clone())?;
        self.right.on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_exit(id, ctx.clone())?;
        self.right.on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_close(id, ctx.clone())?;
        self.right.on_close(id, ctx)
    }
}

impl<A, B, S> Clone for And<A, B, S>
where
    A: Clone,
    B: Clone,
{
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
            _subscriber: PhantomData,
        }
    }
}

impl<A, B, S> fmt::Debug for And<A, B, S>
where
    A: fmt::Debug,
    B: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("And")
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

// === impl Or ===

impl<A, B, S> Or<A, B, S>
where
    A: Filter<S>,
    B: Filter<S>,
{
    /// Combines two [`Filter`]s so that spans and events are enabled if *either* filter
    /// returns `true`.
    ///
    /// # Examples
    ///
    /// Enabling spans and events at the `INFO` level and above, and all spans
    /// and events with a particular target:
    ///
    /// ```ignore
    /// use tracing_subscriber::{
    ///     filter::{filter_fn, LevelFilter, combinator::Or},
    ///     prelude::*,
    /// };
    ///
    /// // Enables spans and events with targets starting with `interesting_target`:
    /// let target_filter = filter_fn(|meta| {
    ///     meta.target().starts_with("interesting_target")
    /// });
    ///
    /// // Enables spans and events with levels `INFO` and below:
    /// let level_filter = LevelFilter::INFO;
    ///
    /// // Combine the two filters together so that a span or event is enabled
    /// // if it is at INFO or lower, or if it has a target starting with
    /// // `interesting_target`.
    /// let filter = Or::new(level_filter, target_filter);
    ///
    /// tracing_subscriber::registry()
    ///     .with(tracing_subscriber::fmt::layer().with_filter(filter))
    ///     .try_init()?;
    ///
    /// // This event will *not* be enabled:
    /// tracing::debug!("an uninteresting event");
    ///
    /// // This event *will* be enabled:
    /// tracing::info!("an uninteresting INFO event");
    ///
    /// // This event *will* be enabled:
    /// tracing::info!(target: "interesting_target", "a very interesting event");
    ///
    /// // This event *will* be enabled:
    /// tracing::debug!(target: "interesting_target", "interesting debug event...");
    /// ```
    ///
    /// Enabling a higher level for a particular target by using `Or` in
    /// conjunction with the [`And`] combinator:
    ///
    /// ```ignore
    /// use tracing_subscriber::{
    ///     filter::{filter_fn, LevelFilter, combinator},
    ///     prelude::*,
    /// };
    ///
    /// // This filter will enable spans and events with targets beginning with
    /// // `my_crate`:
    /// let my_crate = filter_fn(|meta| {
    ///     meta.target().starts_with("my_crate")
    /// });
    ///
    /// // Combine the `my_crate` filter with a `LevelFilter` to produce a filter
    /// // that will enable the `INFO` level and lower for spans and events with
    /// // `my_crate` targets:
    /// let filter = combinator::And::new(my_crate, LevelFilter::INFO);
    ///
    /// // If a span or event *doesn't* have a target beginning with
    /// // `my_crate`, enable it if it has the `WARN` level or lower:
    /// // let filter = combinator::Or::new(filter, LevelFilter::WARN);
    ///
    /// tracing_subscriber::registry()
    ///     .with(tracing_subscriber::fmt::layer().with_filter(filter))
    ///     .try_init()?;
    /// ```
    ///
    /// [`Filter`]: crate::layer::Filter
    #[allow(
        clippy::single_call_fn,
        reason = "public combinator constructor is part of the layer-filter API"
    )]
    pub const fn new(left: A, right: B) -> Self {
        Self {
            left,
            right,
            _subscriber: PhantomData,
        }
    }
}

impl<A, B, S> Filter<S> for Or<A, B, S>
where
    A: Filter<S>,
    B: Filter<S>,
{
    #[inline]
    fn enabled(&self, meta: &Metadata<'_>, cx: &Context<'_, S>) -> SubscriberResult<bool> {
        Ok(self.left.enabled(meta, cx)? || self.right.enabled(meta, cx)?)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        let left_interest = self.left.callsite_enabled(meta)?;
        let right_interest = self.right.callsite_enabled(meta)?;

        // If either filter will always enable the span or event, return `always`.
        if left_interest.is_always() || right_interest.is_always() {
            return Ok(Interest::always());
        }

        // Okay, if either filter will sometimes enable the span or event,
        // return `sometimes`.
        if left_interest.is_sometimes() || right_interest.is_sometimes() {
            return Ok(Interest::sometimes());
        }

        Ok(Interest::never())
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        // If either hint is `None`, return `None`. Otherwise, return the less restrictive.
        let Some(left) = self.left.max_level_hint()? else {
            return Ok(None);
        };
        let Some(right) = self.right.max_level_hint()? else {
            return Ok(None);
        };
        Ok(Some(cmp::max(left, right)))
    }

    #[inline]
    fn event_enabled(
        &self,
        event: &tracing_core::Event<'_>,
        cx: &Context<'_, S>,
    ) -> SubscriberResult<bool> {
        Ok(self.left.event_enabled(event, cx)? || self.right.event_enabled(event, cx)?)
    }

    #[inline]
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        id: Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult<()> {
        self.left.on_new_span(attrs, id, ctx.clone())?;
        self.right.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, id: Id, values: &Record<'_>, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_record(id, values, ctx.clone())?;
        self.right.on_record(id, values, ctx)
    }

    #[inline]
    fn on_enter(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_enter(id, ctx.clone())?;
        self.right.on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_exit(id, ctx.clone())?;
        self.right.on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.left.on_close(id, ctx.clone())?;
        self.right.on_close(id, ctx)
    }
}

impl<A, B, S> Clone for Or<A, B, S>
where
    A: Clone,
    B: Clone,
{
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
            _subscriber: PhantomData,
        }
    }
}

impl<A, B, S> fmt::Debug for Or<A, B, S>
where
    A: fmt::Debug,
    B: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Or")
            .field("left", &self.left)
            .field("right", &self.right)
            .finish()
    }
}

// === impl Not ===

impl<A, S> Not<A, S>
where
    A: Filter<S>,
{
    /// Inverts the result of a [`Filter`].
    ///
    /// If the wrapped filter would enable a span or event, it will be disabled. If
    /// it would disable a span or event, that span or event will be enabled.
    ///
    /// This inverts the values returned by the [`enabled`] and [`callsite_enabled`]
    /// methods on the wrapped filter; it does *not* invert [`event_enabled`], as
    /// filters which do not implement filtering on event field values will return
    /// the default `true` even for events that their [`enabled`] method disables.
    ///
    /// Consider a normal filter defined as:
    ///
    /// ```ignore (pseudo-code)
    /// // for spans
    /// match callsite_enabled() {
    ///     ALWAYS => on_span(),
    ///     SOMETIMES => if enabled() { on_span() },
    ///     NEVER => (),
    /// }
    /// // for events
    /// match callsite_enabled() {
    ///    ALWAYS => on_event(),
    ///    SOMETIMES => if enabled() && event_enabled() { on_event() },
    ///    NEVER => (),
    /// }
    /// ```
    ///
    /// and an inverted filter defined as:
    ///
    /// ```ignore (pseudo-code)
    /// // for spans
    /// match callsite_enabled() {
    ///     ALWAYS => (),
    ///     SOMETIMES => if !enabled() { on_span() },
    ///     NEVER => on_span(),
    /// }
    /// // for events
    /// match callsite_enabled() {
    ///     ALWAYS => (),
    ///     SOMETIMES => if !enabled() { on_event() },
    ///     NEVER => on_event(),
    /// }
    /// ```
    ///
    /// A proper inversion would do `!(enabled() && event_enabled())` (or
    /// `!enabled() || !event_enabled()`), but because of the implicit `&&`
    /// relation between `enabled` and `event_enabled`, it is difficult to
    /// short circuit and not call the wrapped `event_enabled`.
    ///
    /// A combinator which remembers the result of `enabled` in order to call
    /// `event_enabled` only when `enabled() == true` is possible, but requires
    /// additional thread-local mutable state to support a very niche use case.
    //
    //  Also, it'd mean the wrapped layer's `enabled()` always gets called and
    //  globally applied to events where it doesn't today, since we can't know
    //  what `event_enabled` will say until we have the event to call it with.
    ///
    /// [`Filter`]: crate::layer::Filter
    /// [`enabled`]: crate::layer::Filter::enabled
    /// [`event_enabled`]: crate::layer::Filter::event_enabled
    /// [`callsite_enabled`]: crate::layer::Filter::callsite_enabled
    #[allow(
        clippy::single_call_fn,
        reason = "public combinator constructor is part of the layer-filter API"
    )]
    pub const fn new(inner: A) -> Self {
        Self {
            inner,
            _subscriber: PhantomData,
        }
    }
}

impl<A, S> Filter<S> for Not<A, S>
where
    A: Filter<S>,
{
    #[inline]
    fn enabled(&self, meta: &Metadata<'_>, cx: &Context<'_, S>) -> SubscriberResult<bool> {
        Ok(!self.inner.enabled(meta, cx)?)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        Ok(match self.inner.callsite_enabled(meta)? {
            interest if interest.is_always() => Interest::never(),
            interest if interest.is_never() => Interest::always(),
            _ => Interest::sometimes(),
        })
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        // TODO(eliza): figure this out???
        Ok(None)
    }

    #[inline]
    fn event_enabled(
        &self,
        _event: &tracing_core::Event<'_>,
        _cx: &Context<'_, S>,
    ) -> SubscriberResult<bool> {
        // Never disable based on event_enabled; we "disabled" it in `enabled`,
        // so the `not` has already been applied and filtered this not out.
        Ok(true)
    }

    #[inline]
    fn on_new_span(
        &self,
        attrs: &Attributes<'_>,
        id: Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult<()> {
        self.inner.on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, id: Id, values: &Record<'_>, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.inner.on_record(id, values, ctx)
    }

    #[inline]
    fn on_enter(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.inner.on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.inner.on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.inner.on_close(id, ctx)
    }
}

impl<A, S> Clone for Not<A, S>
where
    A: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _subscriber: PhantomData,
        }
    }
}

impl<A, S> fmt::Debug for Not<A, S>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Not").field(&self.inner).finish()
    }
}
