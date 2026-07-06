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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Context;
    use alloc::{sync::Arc, vec::Vec};
    use parking_lot::Mutex;
    use strict_test_support::{TestFailure, ensure, ensure_ok, ensure_some};
    use tracing_core::callsite::Callsite;
    use tracing_core::event::Event;
    use tracing_core::metadata::Kind;
    use tracing_core::subscriber::Interest;
    use tracing_core::Level;

    /// Callsite used by combinator metadata fixtures.
    struct CombinatorCallsite;

    /// Shared callsite used by combinator metadata fixtures.
    static COMBINATOR_CALLSITE: CombinatorCallsite = CombinatorCallsite;

    /// Metadata used for direct filter-combinator calls.
    static COMBINATOR_META: Metadata<'static> = tracing_core::metadata! {
        name: "combinator_test",
        target: "combinator_target",
        level: Level::INFO,
        fields: &["answer"],
        callsite: &COMBINATOR_CALLSITE,
        kind: Kind::EVENT,
    };

    impl Callsite for CombinatorCallsite {
        fn set_interest(&self, _: Interest) {}

        fn metadata(&self) -> &Metadata<'_> {
            &COMBINATOR_META
        }
    }

    /// Callsite-interest polarity used by [`RecordingFilter`].
    #[derive(Clone, Copy, Debug)]
    enum InterestChoice {
        /// Return [`Interest::always`].
        Always,
        /// Return [`Interest::sometimes`].
        Sometimes,
        /// Return [`Interest::never`].
        Never,
    }

    impl InterestChoice {
        /// Converts the test polarity into a tracing-core interest value.
        fn into_interest(self) -> Interest {
            match self {
                Self::Always => Interest::always(),
                Self::Sometimes => Interest::sometimes(),
                Self::Never => Interest::never(),
            }
        }
    }

    /// Shared lifecycle call log used by recording filters.
    type CallLog = Arc<Mutex<Vec<&'static str>>>;

    /// Filter whose return values and lifecycle calls are controlled by tests.
    #[derive(Clone, Debug)]
    struct RecordingFilter {
        /// Prefix written into the call log.
        label: &'static str,
        /// Return value for [`Filter::enabled`].
        enabled: bool,
        /// Return value for [`Filter::event_enabled`].
        event_enabled: bool,
        /// Return value for [`Filter::callsite_enabled`].
        interest: InterestChoice,
        /// Return value for [`Filter::max_level_hint`].
        max_level_hint: Option<LevelFilter>,
        /// Shared lifecycle and predicate call log.
        calls: CallLog,
    }

    impl RecordingFilter {
        /// Records a named filter hook.
        fn record(&self, hook: &'static str) {
            self.calls.lock().push(hook);
        }
    }

    impl<S> Filter<S> for RecordingFilter {
        fn enabled(&self, _meta: &Metadata<'_>, _cx: &Context<'_, S>) -> SubscriberResult<bool> {
            self.record(match self.label {
                "left" => "left.enabled",
                "right" => "right.enabled",
                _ => "inner.enabled",
            });
            Ok(self.enabled)
        }

        fn callsite_enabled(&self, _meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
            self.record(match self.label {
                "left" => "left.callsite",
                "right" => "right.callsite",
                _ => "inner.callsite",
            });
            Ok(self.interest.into_interest())
        }

        fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
            self.record(match self.label {
                "left" => "left.max_level_hint",
                "right" => "right.max_level_hint",
                _ => "inner.max_level_hint",
            });
            Ok(self.max_level_hint)
        }

        fn event_enabled(&self, _event: &Event<'_>, _cx: &Context<'_, S>) -> SubscriberResult<bool> {
            self.record(match self.label {
                "left" => "left.event_enabled",
                "right" => "right.event_enabled",
                _ => "inner.event_enabled",
            });
            Ok(self.event_enabled)
        }

        fn on_new_span(
            &self,
            _attrs: &Attributes<'_>,
            _id: Id,
            _ctx: Context<'_, S>,
        ) -> SubscriberResult<()> {
            self.record(match self.label {
                "left" => "left.on_new_span",
                "right" => "right.on_new_span",
                _ => "inner.on_new_span",
            });
            Ok(())
        }

        fn on_record(
            &self,
            _id: Id,
            _values: &Record<'_>,
            _ctx: Context<'_, S>,
        ) -> SubscriberResult<()> {
            self.record(match self.label {
                "left" => "left.on_record",
                "right" => "right.on_record",
                _ => "inner.on_record",
            });
            Ok(())
        }

        fn on_enter(&self, _id: Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.record(match self.label {
                "left" => "left.on_enter",
                "right" => "right.on_enter",
                _ => "inner.on_enter",
            });
            Ok(())
        }

        fn on_exit(&self, _id: Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.record(match self.label {
                "left" => "left.on_exit",
                "right" => "right.on_exit",
                _ => "inner.on_exit",
            });
            Ok(())
        }

        fn on_close(&self, _id: Id, _ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.record(match self.label {
                "left" => "left.on_close",
                "right" => "right.on_close",
                _ => "inner.on_close",
            });
            Ok(())
        }
    }

    /// Constructs a shared call log.
    fn call_log() -> CallLog {
        Arc::new(Mutex::new(Vec::new()))
    }

    /// Constructs a recording filter used by combinator tests.
    fn recording_filter(
        label: &'static str,
        enabled: bool,
        event_enabled: bool,
        interest: InterestChoice,
        max_level_hint: Option<LevelFilter>,
        calls: &CallLog,
    ) -> RecordingFilter {
        RecordingFilter {
            label,
            enabled,
            event_enabled,
            interest,
            max_level_hint,
            calls: Arc::clone(calls),
        }
    }

    /// Runs every lifecycle hook on a filter.
    fn run_lifecycle_hooks<F>(
        filter: &F,
        attrs: &Attributes<'_>,
        record: &Record<'_>,
        span_id: Id,
        context: &Context<'_, ()>,
        label: &'static str,
    ) -> Result<(), TestFailure>
    where
        F: Filter<()>,
    {
        ensure_ok(filter.on_new_span(attrs, span_id, context.clone()), label)?;
        ensure_ok(filter.on_record(span_id, record, context.clone()), label)?;
        ensure_ok(filter.on_enter(span_id, context.clone()), label)?;
        ensure_ok(filter.on_exit(span_id, context.clone()), label)?;
        ensure_ok(filter.on_close(span_id, context.clone()), label)
    }

    #[test]
    fn boolean_combinators_short_circuit_enabled_and_event_enabled() -> Result<(), TestFailure> {
        let calls = call_log();
        let context = Context::<()>::none();
        let value_set = COMBINATOR_META.fields().value_set(&[]);
        let event = Event::new(&COMBINATOR_META, &value_set);

        let and_filter = And::new(
            recording_filter("left", false, false, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            !ensure_ok(and_filter.enabled(&COMBINATOR_META, &context), "and enabled returns")?,
            "and filter disables when left side disables",
        )?;
        ensure(
            calls.lock().as_slice() == ["left.enabled"],
            "and enabled short-circuits the right side",
        )?;

        calls.lock().clear();
        let or_filter = Or::new(
            recording_filter("left", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", false, false, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            ensure_ok(or_filter.enabled(&COMBINATOR_META, &context), "or enabled returns")?,
            "or filter enables when left side enables",
        )?;
        ensure(
            calls.lock().as_slice() == ["left.enabled"],
            "or enabled short-circuits the right side",
        )?;

        calls.lock().clear();
        let and_events = And::new(
            recording_filter("left", true, false, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            !ensure_ok(
                and_events.event_enabled(&event, &context),
                "and event_enabled returns",
            )?,
            "and event_enabled disables when left side disables",
        )?;
        ensure(
            calls.lock().as_slice() == ["left.event_enabled"],
            "and event_enabled short-circuits the right side",
        )?;

        calls.lock().clear();
        let or_events = Or::new(
            recording_filter("left", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", true, false, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            ensure_ok(or_events.event_enabled(&event, &context), "or event_enabled returns")?,
            "or event_enabled enables when left side enables",
        )?;
        ensure(
            calls.lock().as_slice() == ["left.event_enabled"],
            "or event_enabled short-circuits the right side",
        )
    }

    #[test]
    fn callsite_interest_and_hint_algebra_matches_combinator_contracts() -> Result<(), TestFailure> {
        let calls = call_log();
        let and_never: And<_, _, ()> = And::new(
            recording_filter("left", true, true, InterestChoice::Never, Some(LevelFilter::TRACE), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            ensure_ok(
                and_never.callsite_enabled(&COMBINATOR_META),
                "and never interest returns",
            )?
            .is_never(),
            "and callsite interest short-circuits never",
        )?;
        ensure(
            calls.lock().as_slice() == ["left.callsite"],
            "and callsite never avoids right side",
        )?;

        calls.lock().clear();
        let and_hint: And<_, _, ()> = And::new(
            recording_filter("left", true, true, InterestChoice::Sometimes, Some(LevelFilter::TRACE), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );
        ensure(
            ensure_ok(
                and_hint.callsite_enabled(&COMBINATOR_META),
                "and sometimes interest returns",
            )?
            .is_sometimes(),
            "and keeps left interest when right is always",
        )?;
        ensure(
            ensure_ok(and_hint.max_level_hint(), "and max-level hint returns")?
                == Some(LevelFilter::INFO),
            "and chooses the most restrictive max-level hint",
        )?;

        let or_hint: Or<_, _, ()> = Or::new(
            recording_filter("left", true, true, InterestChoice::Sometimes, Some(LevelFilter::ERROR), &calls),
            recording_filter("right", true, true, InterestChoice::Never, Some(LevelFilter::DEBUG), &calls),
        );
        ensure(
            ensure_ok(or_hint.callsite_enabled(&COMBINATOR_META), "or interest returns")?
                .is_sometimes(),
            "or returns sometimes when either side is sometimes",
        )?;
        ensure(
            ensure_ok(or_hint.max_level_hint(), "or max-level hint returns")?
                == Some(LevelFilter::DEBUG),
            "or chooses the less restrictive max-level hint",
        )?;

        let or_unknown_hint: Or<_, _, ()> = Or::new(
            recording_filter("left", true, true, InterestChoice::Never, None, &calls),
            recording_filter("right", true, true, InterestChoice::Never, Some(LevelFilter::ERROR), &calls),
        );
        ensure(
            ensure_ok(or_unknown_hint.max_level_hint(), "or unknown max-level hint returns")?
                .is_none(),
            "or returns no max-level hint when either side is unknown",
        )
    }

    #[test]
    fn not_inverts_static_interest_and_metadata_enabled_only() -> Result<(), TestFailure> {
        let calls = call_log();
        let context = Context::<()>::none();
        let value_set = COMBINATOR_META.fields().value_set(&[]);
        let event = Event::new(&COMBINATOR_META, &value_set);
        let filter = Not::new(recording_filter(
            "inner",
            true,
            false,
            InterestChoice::Always,
            Some(LevelFilter::ERROR),
            &calls,
        ));

        ensure(
            !ensure_ok(filter.enabled(&COMBINATOR_META, &context), "not enabled returns")?,
            "not inverts metadata enabled",
        )?;
        ensure(
            ensure_ok(filter.callsite_enabled(&COMBINATOR_META), "not interest returns")?
                .is_never(),
            "not inverts always interest to never",
        )?;
        ensure(
            ensure_ok(filter.max_level_hint(), "not max-level hint returns")?.is_none(),
            "not cannot express an inverted max-level hint",
        )?;
        ensure(
            ensure_ok(filter.event_enabled(&event, &context), "not event_enabled returns")?,
            "not leaves event_enabled true after metadata filtering",
        )
    }

    #[test]
    fn and_lifecycle_hooks_forward_to_both_sides() -> Result<(), TestFailure> {
        let calls = call_log();
        let context = Context::<()>::none();
        let value_set = COMBINATOR_META.fields().value_set(&[]);
        let attrs = Attributes::new(&COMBINATOR_META, &value_set);
        let record = Record::new(&value_set);
        let span_id = ensure_some(Id::try_from_u64(1), "nonzero span id")?;
        let filter = And::new(
            recording_filter("left", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );

        run_lifecycle_hooks(&filter, &attrs, &record, span_id, &context, "and forwards hooks")?;

        ensure(
            calls.lock().as_slice()
                == [
                    "left.on_new_span",
                    "right.on_new_span",
                    "left.on_record",
                    "right.on_record",
                    "left.on_enter",
                    "right.on_enter",
                    "left.on_exit",
                    "right.on_exit",
                    "left.on_close",
                    "right.on_close",
                ],
            "and forwards lifecycle hooks to both sides in order",
        )
    }

    #[test]
    fn or_lifecycle_hooks_forward_to_both_sides() -> Result<(), TestFailure> {
        let calls = call_log();
        let context = Context::<()>::none();
        let value_set = COMBINATOR_META.fields().value_set(&[]);
        let attrs = Attributes::new(&COMBINATOR_META, &value_set);
        let record = Record::new(&value_set);
        let span_id = ensure_some(Id::try_from_u64(1), "nonzero span id")?;
        let filter = Or::new(
            recording_filter("left", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
            recording_filter("right", true, true, InterestChoice::Always, Some(LevelFilter::INFO), &calls),
        );

        run_lifecycle_hooks(&filter, &attrs, &record, span_id, &context, "or forwards hooks")?;

        ensure(
            calls.lock().as_slice()
                == [
                    "left.on_new_span",
                    "right.on_new_span",
                    "left.on_record",
                    "right.on_record",
                    "left.on_enter",
                    "right.on_enter",
                    "left.on_exit",
                    "right.on_exit",
                    "left.on_close",
                    "right.on_close",
                ],
            "or forwards lifecycle hooks to both sides in order",
        )
    }

    #[test]
    fn not_lifecycle_hooks_forward_to_the_inner_filter() -> Result<(), TestFailure> {
        let calls = call_log();
        let context = Context::<()>::none();
        let value_set = COMBINATOR_META.fields().value_set(&[]);
        let attrs = Attributes::new(&COMBINATOR_META, &value_set);
        let record = Record::new(&value_set);
        let span_id = ensure_some(Id::try_from_u64(1), "nonzero span id")?;
        let filter = Not::new(recording_filter(
            "inner",
            true,
            true,
            InterestChoice::Always,
            Some(LevelFilter::INFO),
            &calls,
        ));

        run_lifecycle_hooks(&filter, &attrs, &record, span_id, &context, "not forwards hooks")?;

        ensure(
            calls.lock().as_slice()
                == [
                    "inner.on_new_span",
                    "inner.on_record",
                    "inner.on_enter",
                    "inner.on_exit",
                    "inner.on_close",
                ],
            "not forwards lifecycle hooks to the inner filter",
        )
    }
}
