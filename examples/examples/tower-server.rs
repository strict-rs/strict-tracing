//! Example binary for tracing workspace checks.

use std::convert::Infallible;
use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bytes::Bytes;
use futures::future;
use http::Request;
use http::Response;
use http::StatusCode;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tower::Service;
use tower::ServiceBuilder;
use tracing::Span;
use tracing::dispatcher;
use tracing::error;
use tracing::info;
use tracing::info_span;
use tracing_tower::request_span::make;

/// Error type returned by the Tower server example.
type Err = Box<dyn Error + Send + Sync + 'static>;

/// Create the tracing span attached to an inbound request.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the `tracing_tower` request span callback explicit"
)]
fn req_span<A>(req: &Request<A>) -> Span {
  let span = info_span!(
      "request",
      req.method = ?req.method(),
      req.uri = ?req.uri(),
      req.version = ?req.version(),
      req.headers = ?req.headers()
  );
  info!(parent: &span, "received request");
  span
}

/// Root path handled by the example service.
const ROOT: &str = "/";

/// Example Tower service.
#[derive(Copy, Clone, Debug)]
struct Svc;

impl Service<Request<Incoming>> for Svc {
  type Response = Response<Full<Bytes>>;
  type Error = Infallible;
  type Future = future::Ready<Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Ok(()).into()
  }

  fn call(&mut self, req: Request<Incoming>) -> Self::Future {
    let uri = req.uri();
    let response = if uri.path() == ROOT {
      Response::new(Full::new(Bytes::from_static(b"heyo!")))
    } else {
      let mut not_found = Response::new(Full::new(Bytes::new()));
      *not_found.status_mut() = StatusCode::NOT_FOUND;
      not_found
    };

    let span = info_span!(
        "response",
        rsp.status = ?response.status(),
        rsp.version = ?response.version(),
        rsp.headers = ?response.headers()
    );

    dispatcher::get_default(|dispatch| {
      if let (Some(span_id), Some(current)) = (span.id(), dispatch.current_span().ok().and_then(|current| current.id().copied())) {
        let _result = dispatch.record_follows_from(span_id, current);
      }
    });
    let _guard = span.enter();
    info!("sending response");
    future::ok(response)
  }
}

/// Factory for [`Svc`] instances.
#[derive(Copy, Clone, Debug)]
struct MakeSvc;

impl<T> Service<T> for MakeSvc {
  type Response = Svc;
  type Error = io::Error;
  type Future = future::Ready<Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Ok(()).into()
  }

  fn call(&mut self, _: T) -> Self::Future {
    future::ok(Svc)
  }
}

#[tokio::main]
async fn main() -> Result<(), Err> {
  tracing_subscriber::fmt().with_env_filter("tower=trace").try_init()?;

  let mut make_svc = ServiceBuilder::new()
    .timeout(Duration::from_millis(250))
    .layer(make::layer::<_, Svc, _>(req_span))
    .service(MakeSvc);

  let addr: SocketAddr = "127.0.0.1:3000".parse()?;
  let listener = TcpListener::bind(addr).await?;
  info!(message = "listening", addr = ?addr);

  loop {
    let (stream, remote_addr) = listener.accept().await?;
    let io = TokioIo::new(stream);

    let svc = make_svc.call(remote_addr).await?;
    let hyper_svc = TowerToHyperService::new(svc);

    let _task = tokio::spawn(async move {
      let _connection_error = auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, hyper_svc)
        .await
        .inspect_err(|error| error!(%error, "connection error"));
    });
  }
}
