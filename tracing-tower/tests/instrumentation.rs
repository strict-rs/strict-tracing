//! Behavior contracts for `tracing-tower` instrumentation adapters.

#[cfg(test)]
mod tests {
  use core::fmt;
  use core::future::Future;
  use core::pin::Pin;
  use core::task::Context;
  use core::task::Poll;
  use std::error::Error;
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::Ordering;

  use futures::executor::block_on;
  use futures::future;
  use futures::task::noop_waker_ref;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  #[cfg(feature = "tower-layer")]
  use tower_layer::Layer as _;
  use tower_service::Service;
  use tracing::Level;
  use tracing::subscriber::with_default;
  use tracing_mock::expect;
  #[cfg(feature = "http")]
  use tracing_mock::field::ExpectedFields;
  use tracing_mock::subscriber;
  use tracing_tower::InstrumentableService as _;
  #[cfg(feature = "http")]
  use tracing_tower::http::debug_request;
  #[cfg(feature = "http")]
  use tracing_tower::http::error_request;
  #[cfg(feature = "http")]
  use tracing_tower::http::info_request;
  #[cfg(feature = "http")]
  use tracing_tower::http::trace_request;
  #[cfg(feature = "http")]
  use tracing_tower::http::warn_request;
  #[cfg(any(feature = "tower-layer", feature = "tower-make"))]
  use tracing_tower::request_span;
  use tracing_tower::service_span;

  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  struct TestError {
    label: &'static str,
  }

  impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
      formatter.write_str(self.label)
    }
  }

  impl Error for TestError {}

  #[derive(Clone, Debug, Default)]
  struct ReadyService;

  impl Service<&'static str> for ReadyService {
    type Response = &'static str;
    type Error = TestError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      tracing::info!("service ready");
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: &'static str) -> Self::Future {
      tracing::info!(request, "request handled");
      future::ready(Ok(request))
    }
  }

  #[derive(Clone, Debug, Default)]
  struct PolledEventService;

  impl Service<&'static str> for PolledEventService {
    type Response = &'static str;
    type Error = TestError;
    type Future = PolledEventFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: &'static str) -> Self::Future {
      PolledEventFuture {
        request: Some(request)
      }
    }
  }

  #[derive(Debug)]
  struct PolledEventFuture {
    request: Option<&'static str>,
  }

  impl Future for PolledEventFuture {
    type Output = Result<&'static str, TestError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
      let this = self.get_mut();
      let Some(request) = this.request.take() else {
        return Poll::Pending;
      };
      tracing::info!(request, "polled request handled");
      Poll::Ready(Ok(request))
    }
  }

  #[derive(Clone, Debug, Default)]
  struct FailingService;

  impl Service<&'static str> for FailingService {
    type Response = &'static str;
    type Error = TestError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, _request: &'static str) -> Self::Future {
      future::ready(Err(TestError {
        label: "request failed"
      }))
    }
  }

  #[derive(Clone, Debug)]
  struct PendingReadyService {
    ready_polled: Arc<AtomicBool>,
  }

  impl Service<&'static str> for PendingReadyService {
    type Response = &'static str;
    type Error = TestError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      self.ready_polled.store(true, Ordering::Release);
      Poll::Pending
    }

    fn call(&mut self, request: &'static str) -> Self::Future {
      tracing::info!(request, "request handled");
      future::ready(Ok(request))
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Clone, Debug)]
  struct ReadyMakeService {
    service: ReadyService,
  }

  #[cfg(feature = "tower-make")]
  impl Service<&'static str> for ReadyMakeService {
    type Response = ReadyService;
    type Error = TestError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: &'static str) -> Self::Future {
      future::ready(Ok(self.service.clone()))
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Clone, Debug)]
  struct FailingMakeService;

  #[cfg(feature = "tower-make")]
  impl Service<&'static str> for FailingMakeService {
    type Response = ReadyService;
    type Error = TestError;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: &'static str) -> Self::Future {
      future::ready(Err(TestError {
        label: "make failed"
      }))
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Clone, Debug)]
  struct PollingMakeService {
    response: Result<ReadyService, TestError>,
  }

  #[cfg(feature = "tower-make")]
  impl Service<&'static str> for PollingMakeService {
    type Response = ReadyService;
    type Error = TestError;
    type Future = PollingMakeFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: &'static str) -> Self::Future {
      PollingMakeFuture {
        target,
        response: Some(self.response.clone()),
      }
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Debug)]
  struct PollingMakeFuture {
    target:   &'static str,
    response: Option<Result<ReadyService, TestError>>,
  }

  #[cfg(feature = "tower-make")]
  impl Future for PollingMakeFuture {
    type Output = Result<ReadyService, TestError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
      let this = self.get_mut();
      tracing::info!(target = this.target, "make future polled");
      let Some(response) = this.response.take() else {
        return Poll::Pending;
      };
      Poll::Ready(response)
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Clone, Debug)]
  struct RepeatReadyMakeService;

  #[cfg(feature = "tower-make")]
  impl Service<&'static str> for RepeatReadyMakeService {
    type Response = ReadyService;
    type Error = TestError;
    type Future = RepeatReadyMakeFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Poll::Ready(Ok(()))
    }

    fn call(&mut self, _target: &'static str) -> Self::Future {
      RepeatReadyMakeFuture
    }
  }

  #[cfg(feature = "tower-make")]
  #[derive(Debug)]
  struct RepeatReadyMakeFuture;

  #[cfg(feature = "tower-make")]
  impl Future for RepeatReadyMakeFuture {
    type Output = Result<ReadyService, TestError>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
      Poll::Ready(Ok(ReadyService))
    }
  }

  fn make_service_span(_service: &ReadyService) -> tracing::Span {
    tracing::info_span!("service")
  }

  #[cfg(feature = "http")]
  fn ensure_http_constructor_records_span(
    constructor: fn(&http::Request<()>) -> tracing::Span,
    level: Level,
    fields: ExpectedFields,
    context: &'static str,
  ) -> Result<(), TestFailure> {
    let request_span = expect::span().named("request").at_level(level).with_fields(fields);
    let (subscriber, handle) = subscriber::mock().new_span(request_span).run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let request = ensure_ok(
        http::Request::builder()
          .method("POST")
          .uri("/tower")
          .version(http::Version::HTTP_2)
          .header("x-test", "yes")
          .body(()),
        "test HTTP request builds",
      )?;
      let _span = constructor(&request);
      Ok(())
    })?;

    ensure_ok(handle.finished(), context)
  }

  fn poll_ready_once<S, R>(service: &mut S) -> Poll<Result<(), S::Error>>
  where
    S: Service<R>,
  {
    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);
    service.poll_ready(&mut cx)
  }

  fn ensure_ready<S, R>(service: &mut S) -> Result<(), TestFailure>
  where
    S: Service<R>,
    S::Error: fmt::Debug,
  {
    let ready = poll_ready_once::<S, R>(service);
    ensure(matches!(ready, Poll::Ready(Ok(()))), "service reports ready")
  }

  fn call_service<S>(service: &mut S, request: &'static str) -> Result<S::Response, S::Error>
  where
    S: Service<&'static str>,
  {
    block_on(service.call(request))
  }

  #[cfg(feature = "tower-make")]
  fn ensure_second_poll_is_pending<F, T, E>(inner_future: F, context: &'static str) -> Result<(), TestFailure>
  where
    F: Future<Output = Result<T, E>>,
    E: fmt::Debug,
  {
    let mut pinned_future = Box::pin(inner_future);
    let waker = noop_waker_ref();
    let mut cx = Context::from_waker(waker);

    let first = Future::poll(Pin::as_mut(&mut pinned_future), &mut cx);
    ensure(matches!(first, Poll::Ready(Ok(_service))), "first future poll returns a service")?;

    let second = Future::poll(Pin::as_mut(&mut pinned_future), &mut cx);
    ensure(matches!(second, Poll::Pending), context)
  }

  #[test]
  fn trace_requests_enters_request_span_for_call_events() -> Result<(), TestFailure> {
    let request_span = expect::span()
      .named("request")
      .with_fields(expect::field("request").with_value(&"alpha"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("request"))
      .with_fields(expect::field("request").with_value(&"alpha"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(request_span)
      .enter(expect::span().named("request"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
      let mut service = ReadyService.trace_requests(get_span);
      let response = ensure_ok(call_service(&mut service, "alpha"), "request service returns response")?;
      ensure_eq(&response, &"alpha", "request service returns the request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  fn trace_requests_does_not_create_request_span_before_call() -> Result<(), TestFailure> {
    let ready_polled = Arc::new(AtomicBool::new(false));
    let span_requested = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&span_requested);
    let mut service = PendingReadyService {
      ready_polled: Arc::clone(&ready_polled),
    }
    .trace_requests(move |_request: &&'static str| {
      observed.store(true, Ordering::Release);
      tracing::info_span!("request")
    });

    ensure(
      matches!(poll_ready_once::<_, &'static str>(&mut service), Poll::Pending),
      "pending service stays pending",
    )?;
    ensure(ready_polled.load(Ordering::Acquire), "inner readiness was polled")?;
    ensure(
      !span_requested.load(Ordering::Acquire),
      "request span is not requested during readiness",
    )?;

    let response = ensure_ok(call_service(&mut service, "beta"), "pending service still handles call")?;
    ensure_eq(&response, &"beta", "pending service returns the request")?;
    ensure(span_requested.load(Ordering::Acquire), "request span is requested during call")
  }

  #[test]
  fn trace_requests_preserves_inner_service_errors() -> Result<(), TestFailure> {
    let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
    let mut service = FailingService.trace_requests(get_span);
    let error = block_on(service.call("failed")).map_or_else(
      |error| error,
      |_response| TestError {
        label: "unexpected success",
      },
    );
    ensure_eq(&error.label, &"request failed", "request instrumentation preserves inner errors")
  }

  #[test]
  fn trace_service_enters_service_span_for_ready_and_call() -> Result<(), TestFailure> {
    let service_span = expect::span().named("service");
    let ready_event = expect::event().with_ancestry(expect::has_contextual_parent("service"));
    let call_event = expect::event()
      .with_ancestry(expect::has_contextual_parent("service"))
      .with_fields(expect::field("request").with_value(&"gamma"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(service_span)
      .enter(expect::span().named("service"))
      .event(ready_event)
      .exit(expect::span().named("service"))
      .enter(expect::span().named("service"))
      .event(call_event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let mut service = ReadyService.trace_service(make_service_span);
      ensure_ready::<_, &'static str>(&mut service)?;
      let response = ensure_ok(call_service(&mut service, "gamma"), "service-span service returns response")?;
      ensure_eq(&response, &"gamma", "service-span service returns the request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  fn instrument_nests_request_span_inside_service_span() -> Result<(), TestFailure> {
    let service_span = expect::span().named("service");
    let request_span = expect::span()
      .named("request")
      .with_ancestry(expect::has_contextual_parent("service"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("request"))
      .with_fields(expect::field("request").with_value(&"delta"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(service_span)
      .enter(expect::span().named("service"))
      .new_span(request_span)
      .enter(expect::span().named("request"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let mut service = ReadyService.instrument(make_service_span);
      let response = ensure_ok(call_service(&mut service, "delta"), "instrumented service returns response")?;
      ensure_eq(&response, &"delta", "instrumented service returns the request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  fn get_span_closure_receives_request_value() -> Result<(), TestFailure> {
    let observed = Arc::new(AtomicBool::new(false));
    let observed_request = Arc::clone(&observed);
    let mut service = ReadyService.trace_requests(move |request: &&'static str| {
      observed_request.store(*request == "epsilon", Ordering::Release);
      tracing::info_span!("request")
    });

    let response = ensure_ok(call_service(&mut service, "epsilon"), "closure-span service returns response")?;
    ensure_eq(&response, &"epsilon", "closure-span service returns the request")?;
    ensure(observed.load(Ordering::Acquire), "get-span closure observes request")
  }

  #[test]
  fn get_span_accepts_fixed_span_for_all_requests() -> Result<(), TestFailure> {
    let expected_fixed_span = expect::span().named("fixed");
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("fixed"))
      .with_fields(expect::field("request").with_value(&"zeta"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(expected_fixed_span)
      .enter(expect::span().named("fixed"))
      .exit(expect::span().named("fixed"))
      .enter(expect::span().named("fixed"))
      .event(event)
      .exit(expect::span().named("fixed"))
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let actual_fixed_span = tracing::info_span!("fixed");
      let mut service = PolledEventService.trace_requests(actual_fixed_span);
      let response = ensure_ok(call_service(&mut service, "zeta"), "fixed-span service returns response")?;
      ensure_eq(&response, &"zeta", "fixed-span service returns the request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  #[cfg(feature = "tower-layer")]
  fn request_and_service_layers_apply_the_same_wrappers() -> Result<(), TestFailure> {
    let service_span = expect::span().named("service");
    let request_span = expect::span()
      .named("request")
      .with_ancestry(expect::has_contextual_parent("service"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("request"))
      .with_fields(expect::field("request").with_value(&"eta"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(service_span)
      .enter(expect::span().named("service"))
      .new_span(request_span)
      .enter(expect::span().named("request"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
      let request_layer = request_span::layer(get_span);
      let service = request_layer.layer(ReadyService);
      let service_layer = service_span::layer(|_service: &_| tracing::info_span!("service"));
      let mut layered_service = service_layer.layer(service);
      let response = ensure_ok(call_service(&mut layered_service, "eta"), "layered service returns response")?;
      ensure_eq(&response, &"eta", "layered service returns the request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  #[cfg(feature = "tower-make")]
  fn request_make_service_wraps_successful_services_and_preserves_errors() -> Result<(), TestFailure> {
    let request_span = expect::span()
      .named("request")
      .with_fields(expect::field("request").with_value(&"theta"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("request"))
      .with_fields(expect::field("request").with_value(&"theta"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(request_span)
      .enter(expect::span().named("request"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
      let mut make_service = request_span::MakeService::new(
        ReadyMakeService {
          service: ReadyService
        },
        get_span,
      );
      ensure_ready::<_, &'static str>(&mut make_service)?;
      let mut service = ensure_ok(block_on(make_service.call("target")), "request make-service returns service")?;
      let response = ensure_ok(call_service(&mut service, "theta"), "made request service returns response")?;
      ensure_eq(&response, &"theta", "made request service returns the request")
    })?;
    ensure_ok(handle.finished(), "mock expectations should finish")?;

    let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
    let mut failing = request_span::MakeService::new(FailingMakeService, get_span);
    let error = block_on(failing.call("target")).map_or_else(
      |error| error,
      |_made_service| TestError {
        label: "unexpected success",
      },
    );
    ensure_eq(&error.label, &"make failed", "request make-service preserves make errors")
  }

  #[test]
  #[cfg(feature = "tower-make")]
  fn service_make_service_enters_target_span_while_polling_future() -> Result<(), TestFailure> {
    let successful_span = expect::span()
      .named("make service")
      .with_fields(expect::field("target").with_value(&"make-ok"));
    let successful_event = expect::event()
      .with_ancestry(expect::has_contextual_parent("make service"))
      .with_fields(expect::field("target").with_value(&"make-ok"));
    let failing_span = expect::span()
      .named("make service")
      .with_fields(expect::field("target").with_value(&"make-err"));
    let failing_event = expect::event()
      .with_ancestry(expect::has_contextual_parent("make service"))
      .with_fields(expect::field("target").with_value(&"make-err"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(successful_span)
      .enter(expect::span().named("make service"))
      .event(successful_event)
      .exit(expect::span().named("make service"))
      .new_span(failing_span)
      .enter(expect::span().named("make service"))
      .event(failing_event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let successful_get_span = |target: &&'static str| tracing::info_span!("make service", target = *target);
      let mut successful = service_span::make::MakeService::new(
        PollingMakeService {
          response: Ok(ReadyService),
        },
        successful_get_span,
      );
      ensure_ready::<_, &'static str>(&mut successful)?;
      let _successful_service = ensure_ok(block_on(successful.call("make-ok")), "service make-service returns service")?;

      let failing_get_span = |target: &&'static str| tracing::info_span!("make service", target = *target);
      let mut failing = service_span::make::MakeService::new(
        PollingMakeService {
          response: Err(TestError {
            label: "make failed"
          }),
        },
        failing_get_span,
      );
      ensure_ready::<_, &'static str>(&mut failing)?;
      let error = block_on(failing.call("make-err")).map_or_else(
        |error| error,
        |_unexpected_service| TestError {
          label: "unexpected success",
        },
      );
      ensure_eq(&error.label, &"make failed", "service make-service preserves make errors")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  fn cloned_wrappers_preserve_inner_service_behavior() -> Result<(), TestFailure> {
    let request_get_span = |_request: &&'static str| tracing::Span::none();
    let mut request_original = ReadyService.trace_requests(request_get_span);
    let mut request_clone = request_original.clone();

    let original_response = ensure_ok(
      call_service(&mut request_original, "request-original"),
      "original request wrapper returns response",
    )?;
    let cloned_response = ensure_ok(
      call_service(&mut request_clone, "request-clone"),
      "cloned request wrapper returns response",
    )?;
    ensure_eq(
      &original_response,
      &"request-original",
      "original request wrapper preserves request",
    )?;
    ensure_eq(&cloned_response, &"request-clone", "cloned request wrapper preserves request")?;

    let mut service_original = service_span::Service::new(ReadyService, tracing::Span::none());
    let mut service_clone = service_original.clone();
    ensure_ready::<_, &'static str>(&mut service_original)?;
    let service_response = ensure_ok(
      call_service(&mut service_clone, "service-clone"),
      "cloned service wrapper returns response",
    )?;
    ensure_eq(&service_response, &"service-clone", "cloned service wrapper preserves request")?;

    Ok(())
  }

  #[test]
  #[cfg(feature = "tower-make")]
  fn cloned_make_wrappers_preserve_inner_service_behavior() -> Result<(), TestFailure> {
    let make_get_span = |_request: &&'static str| tracing::Span::none();
    let mut request_make_original = request_span::MakeService::new(
      ReadyMakeService {
        service: ReadyService
      },
      make_get_span,
    );
    let mut request_make_clone = request_make_original.clone();
    let _request_original_service = ensure_ok(
      block_on(request_make_original.call("request-make-original")),
      "original request make-service returns service",
    )?;
    let _request_cloned_service = ensure_ok(
      block_on(request_make_clone.call("request-make-clone")),
      "cloned request make-service returns service",
    )?;

    let service_make_get_span = |_target: &&'static str| tracing::Span::none();
    let mut service_make_original = service_span::make::MakeService::new(
      ReadyMakeService {
        service: ReadyService
      },
      service_make_get_span,
    );
    let mut service_make_clone = service_make_original.clone();
    let _service_original_service = ensure_ok(
      block_on(service_make_original.call("service-make-original")),
      "original service make-service returns service",
    )?;
    let _service_cloned_service = ensure_ok(
      block_on(service_make_clone.call("service-make-clone")),
      "cloned service make-service returns service",
    )?;

    Ok(())
  }

  #[test]
  #[cfg(feature = "tower-make")]
  fn make_futures_return_pending_after_wrapped_service_is_taken() -> Result<(), TestFailure> {
    let request_get_span = |_request: &&'static str| tracing::info_span!("request");
    let mut request_make_service = request_span::MakeService::new(RepeatReadyMakeService, request_get_span);
    let request_future = request_make_service.call("request-target");
    ensure_second_poll_is_pending(
      request_future,
      "request make future returns pending after its span factory is consumed",
    )?;

    let service_get_span = |target: &&'static str| tracing::info_span!("service", target = *target);
    let mut service_make_service = service_span::make::MakeService::new(RepeatReadyMakeService, service_get_span);
    let service_future = service_make_service.call("service-target");
    ensure_second_poll_is_pending(
      service_future,
      "service make future returns pending after its service span is consumed",
    )
  }

  #[test]
  #[cfg(all(feature = "tower-layer", feature = "tower-make"))]
  fn request_make_layer_wraps_services_with_request_spans() -> Result<(), TestFailure> {
    let request_span = expect::span()
      .named("request")
      .with_fields(expect::field("request").with_value(&"layer-request"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("request"))
      .with_fields(expect::field("request").with_value(&"layer-request"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(request_span)
      .enter(expect::span().named("request"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let get_span = |request: &&'static str| tracing::span!(Level::INFO, "request", request = *request);
      let make_layer = request_span::make::layer::<&'static str, &'static str, _>(get_span);
      let mut make_service = make_layer.clone().layer(ReadyMakeService {
        service: ReadyService
      });
      ensure_ready::<_, &'static str>(&mut make_service)?;
      let mut service = ensure_ok(block_on(make_service.call("target")), "request make-layer returns service")?;
      let response = ensure_ok(
        call_service(&mut service, "layer-request"),
        "request make-layer service returns response",
      )?;
      ensure_eq(&response, &"layer-request", "request make-layer service returns request")
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[test]
  #[cfg(all(feature = "tower-layer", feature = "tower-make"))]
  fn service_make_layer_enters_target_span_while_polling_future() -> Result<(), TestFailure> {
    let make_span = expect::span()
      .named("make service")
      .with_fields(expect::field("target").with_value(&"layer-target"));
    let event = expect::event()
      .with_ancestry(expect::has_contextual_parent("make service"))
      .with_fields(expect::field("target").with_value(&"layer-target"));
    let (subscriber, handle) = subscriber::mock()
      .new_span(make_span)
      .enter(expect::span().named("make service"))
      .event(event)
      .run_with_handle();

    with_default(subscriber, || -> Result<(), TestFailure> {
      let get_span = |target: &&'static str| tracing::info_span!("make service", target = *target);
      let make_layer = service_span::make::layer::<&'static str, &'static str, _>(get_span);
      let mut make_service = make_layer.clone().layer(PollingMakeService {
        response: Ok(ReadyService),
      });
      ensure_ready::<_, &'static str>(&mut make_service)?;
      let _service = ensure_ok(block_on(make_service.call("layer-target")), "service make-layer returns service")?;
      Ok(())
    })?;

    ensure_ok(handle.finished(), "mock expectations should finish")
  }

  #[cfg(feature = "http")]
  #[test]
  fn http_request_span_constructors_record_expected_fields() -> Result<(), TestFailure> {
    ensure_http_constructor_records_span(
      info_request,
      Level::INFO,
      expect::field("method").and(expect::field("uri")).only(),
      "info HTTP request span is recorded",
    )?;
    ensure_http_constructor_records_span(
      warn_request,
      Level::WARN,
      expect::field("method").and(expect::field("uri")).only(),
      "warn HTTP request span is recorded",
    )?;
    ensure_http_constructor_records_span(
      error_request,
      Level::ERROR,
      expect::field("method").and(expect::field("uri")).only(),
      "error HTTP request span is recorded",
    )?;
    ensure_http_constructor_records_span(
      debug_request,
      Level::DEBUG,
      expect::field("method")
        .and(expect::field("uri"))
        .and(expect::field("version"))
        .only(),
      "debug HTTP request span is recorded",
    )?;
    ensure_http_constructor_records_span(
      trace_request,
      Level::TRACE,
      expect::field("method")
        .and(expect::field("uri"))
        .and(expect::field("version"))
        .and(expect::field("headers"))
        .only(),
      "trace HTTP request span is recorded",
    )
  }
}
