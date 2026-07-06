//! Middleware which instruments a service with a span entered when that service
//! is called.
#[cfg(feature = "tower-make")]
use std::future::Future;
#[cfg(any(feature = "tower-layer", feature = "tower-make"))]
use std::marker::PhantomData;
#[cfg(feature = "tower-make")]
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[cfg(any(feature = "tower-layer", feature = "tower-make"))]
use crate::GetSpan;

#[derive(Debug)]
/// A service wrapper that enters a span while polling readiness and calling.
pub struct Service<S> {
  /// Wrapped service.
  inner: S,
  /// Span entered while the wrapped service is polled or called.
  span:  tracing::Span,
}

#[cfg(feature = "tower-layer")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
pub use self::layer::*;

#[cfg(feature = "tower-layer")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
/// Tower layer support for service-span instrumentation.
mod layer {
  use super::GetSpan;
  use super::PhantomData;
  use super::Service;

  /// Marker for service and request types preserved by the layer.
  type LayerMarker<S, R> = PhantomData<fn(S, R)>;

  #[derive(Debug)]
  /// A Tower layer that instruments a service with a span.
  pub struct Layer<S, R, G = fn(&S) -> tracing::Span>
  where
    G: GetSpan<S>,
    S: tower_service::Service<R>,
  {
    /// Function or span used to create service spans.
    get_span: G,
    /// Preserve the service and request type parameters without storing a request.
    _p:       LayerMarker<S, R>,
  }

  /// Returns a layer that instruments services with spans from `get_span`.
  pub fn layer<S, R, G>(get_span: G) -> Layer<S, R, G>
  where
    G: GetSpan<S>,
    S: tower_service::Service<R>,
  {
    Layer {
      get_span,
      _p: PhantomData,
    }
  }

  // === impl Layer ===

  impl<S, R, G> tower_layer::Layer<S> for Layer<S, R, G>
  where
    G: GetSpan<S>,
    S: tower_service::Service<R>,
  {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
      let span = self.get_span.span_for(&inner);
      Service {
        inner,
        span,
      }
    }
  }

  impl<S, R, G> Clone for Layer<S, R, G>
  where
    G: GetSpan<S> + Clone,
    S: tower_service::Service<R>,
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
/// Make-service adapters that enter spans while creating services.
pub mod make {
  use pin_project_lite::pin_project;

  use super::Context;
  use super::Future;
  use super::GetSpan;
  use super::PhantomData;
  use super::Pin;
  use super::Poll;
  use super::Service;

  /// Marker for make-service target and request types.
  type MakeMarker<T, R> = PhantomData<fn(T, R)>;

  #[derive(Debug)]
  /// A make-service wrapper that enters a span while creating services.
  pub struct MakeService<M, T, R, G = fn(&T) -> tracing::Span>
  where
    G: GetSpan<T>,
  {
    /// Function or span used to create spans for make-service targets.
    get_span: G,
    /// Wrapped make-service.
    inner:    M,
    /// Preserve the target and request type parameters without storing either value.
    _p:       MakeMarker<T, R>,
  }

  pin_project! {
      /// Future returned by [`MakeService`].
      #[derive(Debug)]
      pub struct MakeFuture<F> {
          #[pin]
          inner: F,
          span: Option<tracing::Span>,
      }
  }

  #[cfg(feature = "tower-layer")]
  #[derive(Debug)]
  /// A Tower layer that instruments make-service targets with spans.
  pub struct MakeLayer<T, R, G = fn(&T) -> tracing::Span>
  where
    G: GetSpan<T> + Clone,
  {
    /// Function or span cloned into each make-service wrapper.
    get_span: G,
    /// Preserve the target and request type parameters without storing either value.
    _p:       MakeMarker<T, R>,
  }

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  /// Returns a layer that instruments make-service targets with spans.
  pub fn layer<T, R, G>(get_span: G) -> MakeLayer<T, R, G>
  where
    G: GetSpan<T> + Clone,
  {
    MakeLayer {
      get_span,
      _p: PhantomData,
    }
  }

  // === impl MakeLayer ===

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  impl<M, T, R, G> tower_layer::Layer<M> for MakeLayer<T, R, G>
  where
    M: tower_make::MakeService<T, R>,
    G: GetSpan<T> + Clone,
  {
    type Service = MakeService<M, T, R, G>;

    fn layer(&self, inner: M) -> Self::Service {
      MakeService::new(inner, self.get_span.clone())
    }
  }

  #[cfg(feature = "tower-layer")]
  #[cfg_attr(docsrs, doc(cfg(feature = "tower-layer")))]
  impl<T, R, G> Clone for MakeLayer<T, R, G>
  where
    G: GetSpan<T> + Clone,
  {
    fn clone(&self) -> Self {
      Self {
        get_span: self.get_span.clone(),
        _p:       PhantomData,
      }
    }
  }

  // === impl MakeService ===

  impl<M, T, R, G> tower_service::Service<T> for MakeService<M, T, R, G>
  where
    M: tower_make::MakeService<T, R>,
    G: GetSpan<T>,
  {
    type Response = Service<M::Service>;
    type Error = M::MakeError;
    type Future = MakeFuture<M::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      self.inner.poll_ready(cx)
    }

    fn call(&mut self, target: T) -> Self::Future {
      let span = self.get_span.span_for(&target);
      let inner = self.inner.make_service(target);
      MakeFuture {
        span: Some(span),
        inner,
      }
    }
  }

  impl<F, T, E> Future for MakeFuture<F>
  where
    F: Future<Output = Result<T, E>>,
  {
    type Output = Result<Service<T>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
      let this = self.project();
      let result = {
        let _guard = this.span.as_ref().map(tracing::Span::enter);
        futures::ready!(this.inner.poll(cx))
      };

      let Some(span) = this.span.take() else {
        return Poll::Pending;
      };
      Poll::Ready(result.map(|service| Service::new(service, span)))
    }
  }

  impl<M, T, R, G> MakeService<M, T, R, G>
  where
    G: GetSpan<T>,
  {
    /// Creates a new make-service instrumented with spans from `get_span`.
    pub fn new(inner: M, get_span: G) -> Self {
      Self {
        get_span,
        inner,
        _p: PhantomData,
      }
    }
  }

  impl<M, T, R, G> Clone for MakeService<M, T, R, G>
  where
    M: Clone,
    G: GetSpan<T> + Clone,
  {
    fn clone(&self) -> Self {
      Self::new(self.inner.clone(), self.get_span.clone())
    }
  }
}

// === impl Service ===

impl<S> Service<S> {
  /// Creates a service wrapper that enters `span` around service operations.
  pub const fn new(inner: S, span: tracing::Span) -> Self {
    Self {
      inner,
      span,
    }
  }
}

impl<S, R> tower_service::Service<R> for Service<S>
where
  S: tower_service::Service<R>,
{
  type Response = S::Response;
  type Error = S::Error;
  type Future = S::Future;

  fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    let _enter = self.span.enter();
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, request: R) -> Self::Future {
    let _enter = self.span.enter();
    self.inner.call(request)
  }
}

impl<S> Clone for Service<S>
where
  S: Clone,
{
  fn clone(&self) -> Self {
    Self {
      span:  self.span.clone(),
      inner: self.inner.clone(),
    }
  }
}
