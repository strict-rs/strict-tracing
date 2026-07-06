use core::future::Future;
use core::marker::Sized;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;

use pin_project_lite::pin_project;

use crate::dispatcher;
use crate::dispatcher::Dispatch;
use crate::span::Span;

/// Attaches spans to a [`std::future::Future`].
///
/// Extension trait allowing futures to be
/// instrumented with a `tracing` [span].
///
/// [span]: super::Span
pub trait Instrument: Sized {
  /// Instruments this type with the provided [`Span`], returning an
  /// `Instrumented` wrapper.
  ///
  /// The attached [`Span`] will be [entered] every time the instrumented
  /// [`Future`] is polled or [`Drop`]ped.
  ///
  /// # Examples
  ///
  /// Instrumenting a future:
  ///
  /// ```rust
  /// use tracing::Instrument;
  ///
  /// # async fn doc() {
  /// let my_future = async {
  ///   // ...
  /// };
  ///
  /// my_future.instrument(tracing::info_span!("my_future")).await
  /// # }
  /// ```
  ///
  /// The [`Span::or_current`] combinator can be used in combination with
  /// `instrument` to ensure that the [current span] is attached to the
  /// future if the span passed to `instrument` is [disabled]:
  ///
  /// ```
  /// use tracing::Instrument;
  /// # mod tokio {
  /// #     pub(super) fn spawn(_: impl std::future::Future) {}
  /// # }
  ///
  /// let my_future = async {
  ///   // ...
  /// };
  ///
  /// let outer_span = tracing::info_span!("outer").entered();
  ///
  /// // If the "my_future" span is enabled, then the spawned task will
  /// // be within both "my_future" *and* "outer", since "outer" is
  /// // "my_future"'s parent. However, if "my_future" is disabled,
  /// // the spawned task will *not* be in any span.
  /// tokio::spawn(my_future.instrument(tracing::debug_span!("my_future")));
  ///
  /// // Using `Span::or_current` ensures the spawned task is instrumented
  /// // with the current span, if the new span passed to `instrument` is
  /// // not enabled. This means that if the "my_future"  span is disabled,
  /// // the spawned task will still be instrumented with the "outer" span:
  /// # let my_future = async {};
  /// tokio::spawn(my_future.instrument(tracing::debug_span!("my_future").or_current()));
  /// ```
  ///
  /// [entered]: super::Span::enter()
  /// [`Span::or_current`]: super::Span::or_current()
  /// [current span]: super::Span::current()
  /// [disabled]: super::Span::is_disabled()
  /// [`Future`]: std::future::Future
  fn instrument(self, span: Span) -> Instrumented<Self> {
    Instrumented {
      inner: Some(self),
      span,
    }
  }

  /// Instruments this type with the [current] [`Span`], returning an
  /// `Instrumented` wrapper.
  ///
  /// The attached [`Span`] will be [entered] every time the instrumented
  /// [`Future`] is polled or [`Drop`]ped.
  ///
  /// This can be used to propagate the current span when spawning a new future.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tracing::Instrument;
  ///
  /// # mod tokio {
  /// #     pub(super) fn spawn(_: impl std::future::Future) {}
  /// # }
  /// # async fn doc() {
  /// let span = tracing::info_span!("my_span");
  /// let _enter = span.enter();
  ///
  /// // ...
  ///
  /// let future = async {
  ///   tracing::debug!("this event will occur inside `my_span`");
  ///   // ...
  /// };
  /// tokio::spawn(future.in_current_span());
  /// # }
  /// ```
  ///
  /// [current]: super::Span::current()
  /// [entered]: super::Span::enter()
  /// [`Span`]: crate::Span
  /// [`Future`]: std::future::Future
  #[inline]
  fn in_current_span(self) -> Instrumented<Self> {
    self.instrument(Span::current())
  }
}

/// Extension trait allowing futures to be instrumented with
/// a `tracing` [`Subscriber`](crate::Subscriber).
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub trait WithSubscriber: Sized {
  /// Attaches the provided [`Subscriber`] to this type, returning a
  /// [`WithDispatch`] wrapper.
  ///
  /// The attached [`Subscriber`] will be set as the [default] when the returned
  /// [`Future`] is polled.
  ///
  /// # Examples
  ///
  /// ```
  /// # use tracing::subscriber::NoSubscriber as MySubscriber;
  /// # use tracing::subscriber::NoSubscriber as MyOtherSubscriber;
  /// # async fn docs() {
  /// use tracing::instrument::WithSubscriber;
  ///
  /// // Set the default `Subscriber`
  /// let _default = tracing::subscriber::set_default(MySubscriber::default());
  ///
  /// tracing::info!("this event will be recorded by the default `Subscriber`");
  ///
  /// // Create a different `Subscriber` and attach it to a future.
  /// let other_subscriber = MyOtherSubscriber::default();
  /// let future = async {
  ///   tracing::info!("this event will be recorded by the other `Subscriber`");
  ///   // ...
  /// };
  ///
  /// future
  ///     // Attach the other `Subscriber` to the future before awaiting it
  ///     .with_subscriber(other_subscriber)
  ///     .await;
  ///
  /// // Once the future has completed, we return to the default `Subscriber`.
  /// tracing::info!("this event will be recorded by the default `Subscriber`");
  /// # }
  /// ```
  ///
  /// [`Subscriber`]: super::Subscriber
  /// [default]: dispatcher#setting-the-default-subscriber
  /// [`Future`]: std::future::Future
  fn with_subscriber<S>(self, subscriber: S) -> WithDispatch<Self>
  where
    S: Into<Dispatch>,
  {
    WithDispatch {
      inner:      self,
      dispatcher: subscriber.into(),
    }
  }

  /// Attaches the current [default] [`Subscriber`] to this type, returning a
  /// [`WithDispatch`] wrapper.
  ///
  /// The attached `Subscriber` will be set as the [default] when the returned
  /// [`Future`] is polled.
  ///
  /// This can be used to propagate the current dispatcher context when
  /// spawning a new future that may run on a different thread.
  ///
  /// # Examples
  ///
  /// ```
  /// # mod tokio {
  /// #     pub(super) fn spawn(_: impl std::future::Future) {}
  /// # }
  /// # use tracing::subscriber::NoSubscriber as MySubscriber;
  /// # async fn docs() {
  /// use tracing::instrument::WithSubscriber;
  ///
  /// // Using `set_default` (rather than `set_global_default`) sets the
  /// // default `Subscriber` for *this* thread only.
  /// let _default = tracing::subscriber::set_default(MySubscriber::default());
  ///
  /// let future = async {
  ///   // ...
  /// };
  ///
  /// // If a multi-threaded async runtime is in use, this spawned task may
  /// // run on a different thread, in a different default `Subscriber`'s context.
  /// tokio::spawn(future);
  ///
  /// // However, calling `with_current_subscriber` on the future before
  /// // spawning it, ensures that the current thread's default `Subscriber` is
  /// // propagated to the spawned task, regardless of where it executes:
  /// # let future = async { };
  /// tokio::spawn(future.with_current_subscriber());
  /// # }
  /// ```
  /// [`Subscriber`]: super::Subscriber
  /// [default]: dispatcher#setting-the-default-subscriber
  /// [`Future`]: std::future::Future
  #[inline]
  fn with_current_subscriber(self) -> WithDispatch<Self> {
    WithDispatch {
      inner:      self,
      dispatcher: dispatcher::get_default(Clone::clone),
    }
  }
}

pin_project! {
    /// A [`Future`] that has been instrumented with a `tracing` [`Subscriber`].
    ///
    /// This type is returned by the [`WithSubscriber`] extension trait. See that
    /// trait's documentation for details.
    ///
    /// [`Future`]: std::future::Future
    /// [`Subscriber`]: crate::Subscriber
    #[derive(Clone, Debug)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    pub struct WithDispatch<T> {
        #[pin]
        inner: T,
        dispatcher: Dispatch,
    }
}

pin_project! {
    /// A [`Future`] that has been instrumented with a `tracing` [`Span`].
    ///
    /// This type is returned by the [`Instrument`] extension trait. See that
    /// trait's documentation for details.
    ///
    /// [`Future`]: std::future::Future
    /// [`Span`]: crate::Span
    #[project = InstrumentedProj]
    #[project_ref = InstrumentedProjRef]
    #[derive(Debug, Clone)]
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    pub struct Instrumented<T> {
        #[pin]
        inner: Option<T>,
        span: Span,
    }

    impl<T> PinnedDrop for Instrumented<T> {
        fn drop(this: Pin<&mut Self>) {
            let mut this = this.project();
            if this.inner.as_ref().get_ref().is_some() {
                let _enter = this.span.enter();
                this.inner.set(None);
            }
        }
    }
}

// === impl Instrumented ===

impl<T: Future> Future for Instrumented<T> {
  type Output = T::Output;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      return Poll::Pending;
    };
    let _enter = span.enter();
    inner.poll(cx)
  }
}

impl<T: Sized> Instrument for T {}

impl<T> Instrumented<T> {
  /// Get both a mutable reference to the `Span` that this type is
  /// instrumented by and a pinned mutable reference to the inner value.
  ///
  /// This is useful for implementing poll-type functions on foreign traits.
  #[must_use]
  pub fn span_and_inner_pin_mut(self: Pin<&mut Self>) -> (&mut Span, Option<Pin<&mut T>>) {
    let this = self.project();
    (this.span, this.inner.as_pin_mut())
  }

  /// Borrows the `Span` that this type is instrumented by.
  #[must_use]
  pub const fn span(&self) -> &Span {
    &self.span
  }

  /// Mutably borrows the `Span` that this type is instrumented by.
  #[must_use]
  pub const fn span_mut(&mut self) -> &mut Span {
    &mut self.span
  }

  /// Borrows the wrapped type.
  #[must_use]
  pub const fn inner(&self) -> Option<&T> {
    self.inner.as_ref()
  }

  /// Mutably borrows the wrapped type.
  #[must_use]
  pub const fn inner_mut(&mut self) -> Option<&mut T> {
    self.inner.as_mut()
  }

  /// Get a pinned reference to the wrapped type.
  #[must_use]
  pub fn inner_pin_ref(self: Pin<&Self>) -> Option<Pin<&T>> {
    self.project_ref().inner.as_pin_ref()
  }

  /// Get a pinned mutable reference to the wrapped type.
  #[must_use]
  pub fn inner_pin_mut(self: Pin<&mut Self>) -> Option<Pin<&mut T>> {
    self.project().inner.as_pin_mut()
  }

  /// Consumes the `Instrumented`, returning the wrapped type.
  ///
  /// Note that this drops the span.
  #[must_use]
  pub fn into_inner(mut self) -> Option<T> {
    self.inner.take()
  }
}

// === impl WithDispatch ===

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl<T: Future> Future for WithDispatch<T> {
  type Output = T::Output;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.project();
    let dispatcher = this.dispatcher;
    let future = this.inner;
    let _default = dispatcher::set_default(dispatcher);
    future.poll(cx)
  }
}

#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl<T: Sized> WithSubscriber for T {}

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl<T> WithDispatch<T> {
  /// Borrows the [`Dispatch`] that is entered when this type is polled.
  #[must_use]
  pub const fn dispatcher(&self) -> &Dispatch {
    &self.dispatcher
  }

  /// Borrows the wrapped type.
  #[must_use]
  pub const fn inner(&self) -> &T {
    &self.inner
  }

  /// Mutably borrows the wrapped type.
  #[must_use]
  pub const fn inner_mut(&mut self) -> &mut T {
    &mut self.inner
  }

  /// Get a pinned reference to the wrapped type.
  #[must_use]
  pub fn inner_pin_ref(self: Pin<&Self>) -> Pin<&T> {
    self.project_ref().inner
  }

  /// Get a pinned mutable reference to the wrapped type.
  #[must_use]
  pub fn inner_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
    self.project().inner
  }

  /// Consumes the `Instrumented`, returning the wrapped type.
  ///
  /// Note that this drops the span.
  #[must_use]
  pub fn into_inner(self) -> T {
    self.inner
  }
}
