//! Futures compatibility for [`tracing`].
//!
//! # Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs to collect
//! structured, event-based diagnostic information. This crate provides utilities
//! for using `tracing` to instrument asynchronous code written using futures and
//! async/await.
//!
//! The crate provides the following traits:
//!
//! * [`Instrument`] allows a `tracing` [span] to be attached to a future, sink, stream, or
//!   executor.
//!
//! * [`WithSubscriber`] allows a `tracing` [`Subscriber`] to be attached to a future, sink, stream,
//!   or executor.
//!
//! *Compiler support: [requires `rustc` 1.96+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//!
//! # Feature flags
//!
//! This crate provides a number of feature flags that enable compatibility
//! features with other crates in the asynchronous ecosystem:
//!
//! - `std-future`: Enables compatibility with `std::future::Future`.
//! - `futures-03`: Enables compatibility with version 0.3.x of the `futures` crate's `Spawn` and
//!   `LocalSpawn` traits.
//! - `std`: Depend on the Rust standard library.
//!
//!   `no_std` users may disable this feature with `default-features = false`:
//!
//!   ```toml
//!   [dependencies]
//!   tracing-futures = { version = "0.2.5", default-features = false }
//!   ```
//!
//! The `std-future` and `std` features are enabled by default.
//!
//! [`tracing`]: https://crates.io/crates/tracing
//! [span]: tracing::span!
//! [`Subscriber`]: tracing::subscriber
//!
//! ## Supported Rust Versions
//!
//! Tracing is built against the latest stable release. The minimum supported
//! version is 1.96. The current Tracing version is not guaranteed to build on
//! Rust versions earlier than the minimum supported version.
//!
//! Tracing follows the same compiler support policies as the rest of the Tokio
//! project. The current stable Rust compiler and the three most recent minor
//! versions before it will always be supported. For example, if the current
//! stable compiler version is 1.69, the minimum supported version will not be
//! increased past 1.66, three minor versions prior. Increasing the minimum
//! supported compiler version is not considered a semver breaking change as
//! long as doing so complies with this policy.
#![doc(
  html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
  html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
  issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg), deny(rustdoc::broken_intra_doc_links))]
#[cfg(feature = "std-future")]
use core::future::Future;
#[cfg(feature = "std-future")]
use core::pin::Pin;
#[cfg(feature = "std-future")]
use core::task::Context;
#[cfg(feature = "std-future")]
use core::task::Poll;

#[cfg(feature = "std-future")]
use pin_project_lite::pin_project;
#[cfg(feature = "std")]
use tracing::Dispatch;
use tracing::Span;
#[cfg(feature = "std")]
use tracing::dispatcher;

/// Implementations for `Instrument`ed future executors.
pub mod executor;

/// Extension trait allowing futures, streams, sinks, and executors to be
/// instrumented with a `tracing` [span].
///
/// [span]: mod@tracing::span
pub trait Instrument: Sized {
  /// Instruments this type with the provided [`Span`], returning an
  /// [`Instrumented`] wrapper.
  ///
  /// If the instrumented type is a future, stream, or sink, the attached
  /// [`Span`] will be [entered] every time it is polled or [`Drop`]ped. If
  /// the instrumented type is a future executor, every future spawned on that
  /// executor will be instrumented by the attached [`Span`].
  ///
  /// # Examples
  ///
  /// Instrumenting a future:
  // TODO: ignored until async-await is stable...
  /// ```rust,ignore
  /// use tracing_futures::Instrument;
  ///
  /// # async fn doc() {
  /// let my_future = async {
  ///     // ...
  /// };
  ///
  /// my_future
  ///     .instrument(tracing::info_span!("my_future"))
  ///     .await
  /// # }
  /// ```
  ///
  /// [entered]: Span::enter()
  fn instrument(self, span: Span) -> Instrumented<Self> {
    Instrumented {
      inner: Some(self),
      span,
    }
  }

  /// Instruments this type with the [current] [`Span`], returning an
  /// [`Instrumented`] wrapper.
  ///
  /// If the instrumented type is a future, stream, or sink, the attached
  /// [`Span`] will be [entered] every time it is polled or [`Drop`]ped. If
  /// the instrumented type is a future executor, every future spawned on that
  /// executor will be instrumented by the attached [`Span`].
  ///
  /// This can be used to propagate the current span when spawning a new future.
  ///
  /// # Examples
  // TODO: ignored until async-await is stable...
  /// ```rust,ignore
  /// use tracing_futures::Instrument;
  ///
  /// # async fn doc() {
  /// let span = tracing::info_span!("my_span");
  /// let _enter = span.enter();
  ///
  /// // ...
  ///
  /// let future = async {
  ///     tracing::debug!("this event will occur inside `my_span`");
  ///     // ...
  /// };
  /// tokio::spawn(future.in_current_span());
  /// # }
  /// ```
  ///
  /// [current]: Span::current()
  /// [entered]: Span::enter()
  #[inline]
  fn in_current_span(self) -> Instrumented<Self> {
    self.instrument(Span::current())
  }
}

/// Extension trait allowing futures, streams, and sinks to be instrumented with
/// a `tracing` [`Subscriber`].
///
/// [`Subscriber`]: tracing::Subscriber
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub trait WithSubscriber: Sized {
  /// Attaches the provided [`Subscriber`] to this type, returning a
  /// `WithDispatch` wrapper.
  ///
  /// When the wrapped type is a future, stream, or sink, the attached
  /// subscriber will be set as the [default] while it is being polled.
  /// When the wrapped type is an executor, the subscriber will be set as the
  /// default for any futures spawned on that executor.
  ///
  /// [`Subscriber`]: tracing::Subscriber
  /// [default]: tracing::dispatcher#setting-the-default-subscriber
  fn with_subscriber<S>(self, subscriber: S) -> WithDispatch<Self>
  where
    S: Into<Dispatch>,
  {
    WithDispatch {
      inner:    self,
      dispatch: subscriber.into(),
    }
  }

  /// Attaches the current [default] [`Subscriber`] to this type, returning a
  /// `WithDispatch` wrapper.
  ///
  /// When the wrapped type is a future, stream, or sink, the attached
  /// subscriber will be set as the [default] while it is being polled.
  /// When the wrapped type is an executor, the subscriber will be set as the
  /// default for any futures spawned on that executor.
  ///
  /// This can be used to propagate the current dispatcher context when
  /// spawning a new future.
  ///
  /// [`Subscriber`]: tracing::Subscriber
  /// [default]: tracing::dispatcher#setting-the-default-subscriber
  #[inline]
  fn with_current_subscriber(self) -> WithDispatch<Self> {
    WithDispatch {
      inner:    self,
      dispatch: dispatcher::get_default(Clone::clone),
    }
  }
}

#[cfg(feature = "std-future")]
pin_project! {
    /// A future, stream, sink, or executor that has been instrumented with a `tracing` span.
    #[project = InstrumentedProj]
    #[project_ref = InstrumentedProjRef]
    #[derive(Debug, Clone)]
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

/// A future, stream, sink, or executor that has been instrumented with a `tracing` span.
#[cfg(not(feature = "std-future"))]
#[derive(Debug, Clone)]
pub struct Instrumented<T> {
  inner: Option<T>,
  span:  Span,
}

#[cfg(not(feature = "std-future"))]
impl<T> Drop for Instrumented<T> {
  fn drop(&mut self) {
    if self.inner.is_some() {
      let _enter = self.span.enter();
      let _inner = self.inner.take();
    }
  }
}

#[cfg(all(feature = "std", feature = "std-future"))]
pin_project! {
    /// A future, stream, sink, or executor that has been instrumented with a
    /// `tracing` subscriber.
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    #[derive(Clone, Debug)]
    pub struct WithDispatch<T> {
        #[pin]
        inner: T,
        dispatch: Dispatch,
    }
}

/// A future, stream, sink, or executor that has been instrumented with a
/// `tracing` subscriber.
#[cfg(all(feature = "std", not(feature = "std-future")))]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
#[derive(Clone, Debug)]
pub struct WithDispatch<T> {
  inner:    T,
  dispatch: Dispatch,
}

impl<T: Sized> Instrument for T {}

#[cfg(feature = "std-future")]
#[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
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

#[cfg(all(feature = "futures-03", feature = "std-future"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "futures-03", feature = "std-future"))))]
impl<T: futures::Stream> futures::Stream for Instrumented<T> {
  type Item = T::Item;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      return Poll::Ready(None);
    };
    let _enter = span.enter();
    T::poll_next(inner, cx)
  }
}

#[cfg(all(feature = "futures-03", feature = "std-future"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "futures-03", feature = "std-future"))))]
impl<I, T> futures::Sink<I> for Instrumented<T>
where
  T: futures::Sink<I>,
{
  type Error = T::Error;

  fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      return Poll::Ready(Ok(()));
    };
    let _enter = span.enter();
    T::poll_ready(inner, cx)
  }

  fn start_send(self: Pin<&mut Self>, outgoing: I) -> Result<(), Self::Error> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      let _outgoing = outgoing;
      return Ok(());
    };
    let _enter = span.enter();
    T::start_send(inner, outgoing)
  }

  fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      return Poll::Ready(Ok(()));
    };
    let _enter = span.enter();
    T::poll_flush(inner, cx)
  }

  fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    let (span, Some(inner)) = self.span_and_inner_pin_mut() else {
      return Poll::Ready(Ok(()));
    };
    let _enter = span.enter();
    T::poll_close(inner, cx)
  }
}

impl<T> Instrumented<T> {
  /// Get both a mutable reference to the `Span` that this type is
  /// instrumented by and a pinned mutable reference to the inner value.
  ///
  /// This is useful for implementing poll-type functions on foreign traits.
  #[must_use]
  #[cfg(feature = "std-future")]
  #[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
  pub fn span_and_inner_pin_mut(self: Pin<&mut Self>) -> (&mut Span, Option<Pin<&mut T>>) {
    let this = self.project();
    (this.span, this.inner.as_pin_mut())
  }

  /// Borrows the `Span` that this type is instrumented by.
  pub const fn span(&self) -> &Span {
    &self.span
  }

  /// Mutably borrows the `Span` that this type is instrumented by.
  pub const fn span_mut(&mut self) -> &mut Span {
    &mut self.span
  }

  /// Borrows the wrapped type.
  pub const fn inner(&self) -> Option<&T> {
    self.inner.as_ref()
  }

  /// Mutably borrows the wrapped type.
  pub const fn inner_mut(&mut self) -> Option<&mut T> {
    self.inner.as_mut()
  }

  /// Get a pinned reference to the wrapped type.
  #[must_use]
  #[cfg(feature = "std-future")]
  #[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
  pub fn inner_pin_ref(self: Pin<&Self>) -> Option<Pin<&T>> {
    self.project_ref().inner.as_pin_ref()
  }

  /// Get a pinned mutable reference to the wrapped type.
  #[must_use]
  #[cfg(feature = "std-future")]
  #[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
  pub fn inner_pin_mut(self: Pin<&mut Self>) -> Option<Pin<&mut T>> {
    self.project().inner.as_pin_mut()
  }

  /// Consumes the `Instrumented`, returning the wrapped type.
  ///
  /// Note that this drops the span.
  pub fn into_inner(mut self) -> Option<T> {
    self.inner.take()
  }
}

#[cfg(feature = "std")]
impl<T: Sized> WithSubscriber for T {}

#[cfg(all(feature = "std-future", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "std-future", feature = "std"))))]
impl<T: Future> Future for WithDispatch<T> {
  type Output = T::Output;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let this = self.project();
    let dispatch = this.dispatch;
    let future = this.inner;
    dispatcher::with_default(dispatch, || future.poll(cx))
  }
}

#[cfg(feature = "std")]
impl<T> WithDispatch<T> {
  /// Wrap a future, stream, sink or executor with the same subscriber as this
  /// `WithDispatch`.
  pub fn with_dispatch<U>(&self, inner: U) -> WithDispatch<U> {
    WithDispatch {
      dispatch: self.dispatch.clone(),
      inner,
    }
  }

  /// Borrows the `Dispatch` that this type is instrumented by.
  pub const fn dispatch(&self) -> &Dispatch {
    &self.dispatch
  }

  /// Get a pinned reference to the wrapped type.
  #[must_use]
  #[cfg(feature = "std-future")]
  #[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
  pub fn inner_pin_ref(self: Pin<&Self>) -> Pin<&T> {
    self.project_ref().inner
  }

  /// Get a pinned mutable reference to the wrapped type.
  #[must_use]
  #[cfg(feature = "std-future")]
  #[cfg_attr(docsrs, doc(cfg(feature = "std-future")))]
  pub fn inner_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
    self.project().inner
  }

  /// Borrows the wrapped type.
  pub const fn inner(&self) -> &T {
    &self.inner
  }

  /// Mutably borrows the wrapped type.
  pub const fn inner_mut(&mut self) -> &mut T {
    &mut self.inner
  }

  /// Consumes the `WithDispatch`, returning the wrapped type.
  pub fn into_inner(self) -> T {
    self.inner
  }
}

#[cfg(test)]
mod tests {

  #[cfg(all(feature = "futures-03", feature = "std-future"))]
  mod futures_03_tests {
    use futures::FutureExt as _;
    use futures::SinkExt as _;
    use futures::StreamExt as _;
    use futures::future;
    use futures::sink;
    use futures::stream;
    use strict_test_support::TestFailure;
    use strict_test_support::ensure;
    use strict_test_support::ensure_ok;
    use strict_test_support::ensure_some;
    use tracing::subscriber::with_default;
    use tracing_mock::expect;
    use tracing_mock::subscriber;

    use crate::Instrument;

    #[test]
    fn stream_enter_exit_is_reasonable() -> Result<(), TestFailure> {
      let (subscriber, handle) = subscriber::mock()
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .close_span(expect::span().named("foo"))
        .run_with_handle();
      with_default(subscriber, || {
        ensure_some(
          Instrument::instrument(stream::iter(&[1, 2, 3]), tracing::trace_span!("foo"))
            .for_each(|_| future::ready(()))
            .now_or_never(),
          "instrumented futures 0.3 stream resolves synchronously",
        )?;
        Ok::<(), TestFailure>(())
      })?;
      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }

    #[test]
    fn sink_enter_exit_is_reasonable() -> Result<(), TestFailure> {
      let (subscriber, handle) = subscriber::mock()
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .enter(expect::span().named("foo"))
        .exit(expect::span().named("foo"))
        .close_span(expect::span().named("foo"))
        .run_with_handle();
      with_default(subscriber, || {
        let output = ensure_some(
          Instrument::instrument(sink::drain(), tracing::trace_span!("foo"))
            .send(1_u8)
            .now_or_never(),
          "instrumented futures 0.3 sink resolves synchronously",
        )?;
        ensure(output.is_ok(), "instrumented futures 0.3 sink send succeeds")
      })?;
      ensure_ok(handle.finished(), "mock expectations should finish")?;
      Ok(())
    }
  }
}
