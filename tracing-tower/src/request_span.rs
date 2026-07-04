//! Middleware which instruments each request passing through a service with a new span.
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::future::Future;
use tracing::Instrument as _;
use tracing::instrument::Instrumented;

use super::GetSpan;

#[derive(Debug)]
/// A service wrapper that creates a new span for each request.
pub struct Service<S, R, G = fn(&R) -> tracing::Span>
where
  S: tower_service::Service<R>,
  G: GetSpan<R>,
{
  /// Function or span used to create request spans.
  get_span: G,
  /// Wrapped service.
  inner:    S,
  /// Preserve the request type parameter without storing a request.
  _p:       PhantomData<fn(R)>,
}

#[cfg(feature = "tower-layer")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
pub use self::layer::*;

#[cfg(feature = "tower-layer")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
/// Tower layer support for request-span instrumentation.
mod layer {
  use super::GetSpan;
  use super::PhantomData;
  use super::Service;

  #[derive(Debug)]
  /// A Tower layer that applies request-span instrumentation.
  pub struct Layer<R, G = fn(&R) -> tracing::Span>
  where
    G: GetSpan<R> + Clone,
  {
    /// Function or span used to create request spans.
    get_span: G,
    /// Preserve the request type parameter without storing a request.
    _p:       PhantomData<fn(R)>,
  }

  /// Returns a layer that instruments each request with `get_span`.
  pub fn layer<R, G>(get_span: G) -> Layer<R, G>
  where
    G: GetSpan<R> + Clone,
  {
    Layer {
      get_span,
      _p: PhantomData,
    }
  }

  // === impl Layer ===
  impl<S, R, G> tower_layer::Layer<S> for Layer<R, G>
  where
    S: tower_service::Service<R>,
    G: GetSpan<R> + Clone,
  {
    type Service = Service<S, R, G>;

    fn layer(&self, service: S) -> Self::Service {
      Service::new(service, self.get_span.clone())
    }
  }

  impl<R, G> Clone for Layer<R, G>
  where
    G: GetSpan<R> + Clone,
  {
    fn clone(&self) -> Self {
      Self {
        get_span: self.get_span.clone(),
        _p:       PhantomData,
      }
    }
  }
}

#[cfg(feature = "tower-make")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-make")))]
pub use self::make::MakeService;

#[cfg(feature = "tower-make")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-make")))]
/// Make-service adapters that add request-span instrumentation.
pub mod make {
  use pin_project_lite::pin_project;

  use super::Context;
  use super::Future;
  use super::GetSpan;
  use super::PhantomData;
  use super::Pin;
  use super::Poll;
  use super::Service;

  #[derive(Debug)]
  /// A make-service wrapper that instruments produced services by request.
  pub struct MakeService<S, R, G = fn(&R) -> tracing::Span> {
    /// Function or span cloned into each produced request-instrumenting service.
    get_span: G,
    /// Wrapped make-service.
    inner:    S,
    /// Preserve the request type parameter without storing a request.
    _p:       PhantomData<fn(R)>,
  }

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  #[derive(Debug)]
  /// A Tower layer that applies request-span instrumentation to make-services.
  pub struct MakeLayer<R, T, G = fn(&R) -> tracing::Span>
  where
    G: GetSpan<R> + Clone,
  {
    /// Function or span cloned into each produced request-instrumenting service.
    get_span: G,
    /// Preserve the target and request type parameters without storing either value.
    _p:       PhantomData<fn(T, R)>,
  }

  pin_project! {
      /// Future returned by [`MakeService`].
      #[derive(Debug)]
      pub struct MakeFuture<F, R, G> {
          get_span: Option<G>,
          #[pin]
          inner: F,
          _p: PhantomData<fn(R)>,
      }
  }

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  /// Returns a make-service layer that instruments each produced service by request.
  pub fn layer<R, T, G>(get_span: G) -> MakeLayer<R, T, G>
  where
    G: GetSpan<R> + Clone,
  {
    MakeLayer {
      get_span,
      _p: PhantomData,
    }
  }

  // === impl MakeLayer ===

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  impl<S, R, G, T> tower_layer::Layer<S> for MakeLayer<R, T, G>
  where
    S: tower_make::MakeService<T, R>,
    G: GetSpan<R> + Clone,
  {
    type Service = MakeService<S, R, G>;

    fn layer(&self, inner: S) -> Self::Service {
      MakeService::new(inner, self.get_span.clone())
    }
  }

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  impl<R, T, G> Clone for MakeLayer<R, T, G>
  where
    G: GetSpan<R> + Clone,
  {
    fn clone(&self) -> Self {
      Self {
        get_span: self.get_span.clone(),
        _p:       PhantomData,
      }
    }
  }

  // === impl MakeService ===

  impl<S, R, G, T> tower_service::Service<T> for MakeService<S, R, G>
  where
    S: tower_make::MakeService<T, R>,
    G: GetSpan<R> + Clone,
  {
    type Response = Service<S::Service, R, G>;
    type Error = S::MakeError;
    type Future = MakeFuture<S::Future, R, G>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      self.inner.poll_ready(cx)
    }

    fn call(&mut self, target: T) -> Self::Future {
      let inner = self.inner.make_service(target);
      let get_span = Some(self.get_span.clone());
      MakeFuture {
        get_span,
        inner,
        _p: PhantomData,
      }
    }
  }

  impl<S, R, G> MakeService<S, R, G>
  where
    G: GetSpan<R> + Clone,
  {
    /// Creates a new request-instrumenting make-service.
    #[allow(
      clippy::single_call_fn,
      reason = "public constructor is shared by direct make-service users and the tower Layer adapter"
    )]
    pub fn new<T>(inner: S, get_span: G) -> Self
    where
      S: tower_make::MakeService<T, R>,
    {
      Self {
        get_span,
        inner,
        _p: PhantomData,
      }
    }
  }

  impl<S, R, G> Clone for MakeService<S, R, G>
  where
    G: GetSpan<R> + Clone,
    S: Clone,
  {
    fn clone(&self) -> Self {
      Self {
        get_span: self.get_span.clone(),
        inner:    self.inner.clone(),
        _p:       PhantomData,
      }
    }
  }

  impl<F, R, G, S, E> Future for MakeFuture<F, R, G>
  where
    F: Future<Output = Result<S, E>>,
    S: tower_service::Service<R>,
    G: GetSpan<R> + Clone,
  {
    type Output = Result<Service<S, R, G>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
      let this = self.project();
      let result = futures::ready!(this.inner.poll(cx));
      let Some(get_span) = this.get_span.take() else {
        return Poll::Pending;
      };
      Poll::Ready(result.map(|inner| Service {
        get_span,
        inner,
        _p: PhantomData,
      }))
    }
  }
}

// === impl Service ===

impl<S, R, G> tower_service::Service<R> for Service<S, R, G>
where
  S: tower_service::Service<R>,
  G: GetSpan<R> + Clone,
{
  type Response = S::Response;
  type Error = S::Error;
  type Future = Instrumented<S::Future>;

  fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, request: R) -> Self::Future {
    let span = self.get_span.span_for(&request);
    let _enter = span.enter();
    self.inner.call(request).instrument(span.clone())
  }
}

impl<S, R, G> Clone for Service<S, R, G>
where
  S: tower_service::Service<R> + Clone,
  G: GetSpan<R> + Clone,
{
  fn clone(&self) -> Self {
    Self {
      get_span: self.get_span.clone(),
      inner:    self.inner.clone(),
      _p:       PhantomData,
    }
  }
}

impl<S, R, G> Service<S, R, G>
where
  S: tower_service::Service<R>,
  G: GetSpan<R> + Clone,
{
  /// Creates a new request-instrumenting service.
  pub fn new(inner: S, get_span: G) -> Self {
    Self {
      get_span,
      inner,
      _p: PhantomData,
    }
  }
}
