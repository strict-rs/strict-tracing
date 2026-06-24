//! Tower service middleware for creating and entering `tracing` spans.
//!
//! This crate provides adapters for instrumenting services, requests, and
//! make-service futures with spans derived from either a closure or an existing
//! [`tracing::Span`].

#![cfg_attr(docsrs, feature(doc_cfg), deny(rustdoc::broken_intra_doc_links))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
    html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
    issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
use std::fmt;
use tower_service::Service;
use tracing::Level;

pub mod request_span;
/// Middleware for entering a span while polling or calling a service.
pub mod service_span;

#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
/// Helpers for building spans from HTTP requests.
pub mod http;

/// A service instrumented with both service-level and request-level spans.
pub type InstrumentedService<S, R> = service_span::Service<request_span::Service<S, R>>;

/// Extension methods for adding `tracing` spans to Tower services.
pub trait InstrumentableService<Request>
where
    Self: Service<Request> + Sized,
{
    /// Instruments the service with a service span and per-request spans.
    fn instrument<G>(self, svc_span: G) -> InstrumentedService<Self, Request>
    where
        G: GetSpan<Self>,
        Request: fmt::Debug,
    {
        let req_span: fn(&Request) -> tracing::Span =
            |request| tracing::span!(Level::TRACE, "request", ?request);
        let svc_span = svc_span.span_for(&self);
        self.trace_requests(req_span).trace_service(svc_span)
    }

    /// Instruments each request handled by this service with a new span.
    fn trace_requests<G>(self, get_span: G) -> request_span::Service<Self, Request, G>
    where
        G: GetSpan<Request> + Clone,
    {
        request_span::Service::new(self, get_span)
    }

    /// Instruments this service with a span entered around service calls.
    fn trace_service<G>(self, get_span: G) -> service_span::Service<Self>
    where
        G: GetSpan<Self>,
    {
        let span = get_span.span_for(&self);
        service_span::Service::new(self, span)
    }
}

impl<S, R> InstrumentableService<R> for S where S: Service<R> + Sized {}

/// Produces a span for a target value.
pub trait GetSpan<T>: sealed::Sealed<T> {
    /// Returns the span that should be used to instrument `target`.
    fn span_for(&self, target: &T) -> tracing::Span;
}

impl<T, F> sealed::Sealed<T> for F where F: Fn(&T) -> tracing::Span {}

impl<T, F> GetSpan<T> for F
where
    F: Fn(&T) -> tracing::Span,
{
    #[inline]
    fn span_for(&self, target: &T) -> tracing::Span {
        (self)(target)
    }
}

impl<T> sealed::Sealed<T> for tracing::Span {}

impl<T> GetSpan<T> for tracing::Span {
    #[inline]
    fn span_for(&self, _: &T) -> tracing::Span {
        self.clone()
    }
}

mod sealed {
    pub trait Sealed<T = ()> {}
}
