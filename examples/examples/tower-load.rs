//! A demo showing how filtering on values and dynamic filter reloading can be
//! used together to help make sense of complex or noisy traces.
//!
//! This example runs a simple HTTP server that implements highly advanced,
//! cloud-native "character repetition as a service", on port 3000. The server's
//! `GET /${CHARACTER}` route will respond with a string containing that
//! character repeated up to the requested `Content-Length`. A load generator
//! runs in the background and constantly sends requests for various characters
//! to be repeated.
//!
//! As the load generator's logs indicate, the server will sometimes return
//! errors, including HTTP 500s! Because the logs at high load are so noisy,
//! tracking down the root cause of the errors can be difficult, if not
//! impossible. Since the character-repetition service is absolutely
//! mission-critical to our organization, we have to determine what is causing
//! these errors as soon as possible!
//!
//! Fortunately, an admin service running on port 3001 exposes a `PUT /filter`
//! route that can be used to change the trace filter for the format subscriber.
//! By dynamically changing the filter we can try to track down the cause of the
//! error.
//!
//! As a hint: all spans and events from the load generator have the "gen" target
#![deny(rust_2018_idioms)]

use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bytes::Bytes;
use futures::Future;
use futures::future::Ready;
use futures::future::{
  self,
};
use http::Method;
use http::Request;
use http::Response;
use http::StatusCode;
use http::header;
use http_body_util::BodyExt as _;
use http_body_util::Empty;
use http_body_util::Full;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use rand::RngExt as _;
use tokio::net::TcpListener;
use tokio::time;
use tokio::try_join;
use tower::Service;
use tower::ServiceBuilder;
use tower::ServiceExt as _;
use tracing::Instrument as _;
use tracing::Level;
use tracing::Span;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::info_span;
use tracing::span;
use tracing::trace;
use tracing::warn;
use tracing::{
  self,
};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::reload::Handle;
use tracing_tower::GetSpan;
use tracing_tower::request_span;
use tracing_tower::request_span::make;

/// Error type shared by the example services and load generator.
type BoxError = Box<dyn Error + Send + Sync + 'static>;
/// Boxed response body used by the Tower and Hyper services.
type ResponseBody = BoxBody<Bytes, Infallible>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
  let builder = tracing_subscriber::fmt()
    .with_env_filter("info,tower_load=debug")
    .with_filter_reloading();
  let handle = builder.reload_handle();
  builder.try_init()?;

  let addr = "[::1]:3000".parse::<SocketAddr>()?;
  let admin_addr = "[::1]:3001".parse::<SocketAddr>()?;

  let admin = AdminSvc {
    handle,
  };

  let make_svc = ServiceBuilder::new().layer(make::layer::<_, Svc, _>(req_span)).service(MakeSvc);

  let res = try_join!(
    tokio::spawn(async move { load_gen(&addr).await }),
    tokio::spawn(async move { load_gen(&addr).await }),
    tokio::spawn(async move { load_gen(&addr).await }),
    tokio::spawn(async move { serve(&addr, make_svc).await }),
    tokio::spawn(async move { serve_admin(&admin_addr, admin).await }),
  );

  match res {
    Ok(_) => info!("load generator exited successfully"),
    Err(error) => {
      error!(error = ?error, "load generator failed");
    }
  }
  Ok(())
}

/// Serves the character repetition endpoint with request spans.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the request server loop separate from load-test setup"
)]
async fn serve<G>(addr: &SocketAddr, mut make_svc: make::MakeService<MakeSvc, Request<Incoming>, G>) -> Result<(), BoxError>
where
  G: GetSpan<Request<Incoming>> + Clone + Send + 'static,
{
  let listener = TcpListener::bind(*addr).await?;
  loop {
    let (stream, remote_addr) = listener.accept().await?;
    let io = TokioIo::new(stream);
    let svc = make_svc.call(remote_addr).await?;
    let hyper_svc = TowerToHyperService::new(svc);
    let _task = tokio::spawn(async move {
      if let Err(error) = auto::Builder::new(TokioExecutor::new()).serve_connection(io, hyper_svc).await {
        error!(error = %error, "connection error");
      }
    });
  }
}

/// Serves the filter reload endpoint used while the load test is running.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the admin reload server loop separate from the request server"
)]
async fn serve_admin<S>(addr: &SocketAddr, admin: AdminSvc<S>) -> Result<(), BoxError>
where
  S: tracing::Subscriber + 'static,
{
  let listener = TcpListener::bind(*addr).await?;
  loop {
    let (stream, _remote_addr) = listener.accept().await?;
    let io = TokioIo::new(stream);
    let hyper_svc = TowerToHyperService::new(admin.clone());
    let _task = tokio::spawn(async move {
      if let Err(error) = auto::Builder::new(TokioExecutor::new()).serve_connection(io, hyper_svc).await {
        error!(error = %error, "admin connection error");
      }
    });
  }
}

/// Tower service that repeats the requested path character.
#[derive(Clone)]
struct Svc;
impl Service<Request<Incoming>> for Svc {
  type Response = Response<ResponseBody>;
  type Error = BoxError;
  type Future = Ready<Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, req: Request<Incoming>) -> Self::Future {
    let response = match Self::handle_request(&req) {
      Ok(body) => {
        trace!("sending response");
        rsp(StatusCode::OK, body)
      }
      Err(error) => {
        trace!(rsp.error = %error);
        let error_message = error.to_string();
        let status = match error {
          HandleError::BadPath => {
            warn!(rsp.status = %StatusCode::NOT_FOUND);
            StatusCode::NOT_FOUND
          }
          HandleError::NoContentLength | HandleError::BadRequest(_) => StatusCode::BAD_REQUEST,
          HandleError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
        };
        rsp(status, error_message)
      }
    };
    future::ready(response)
  }
}

impl Svc {
  /// Builds the response body for a single character-repetition request.
  #[allow(
    clippy::single_call_fn,
    reason = "keeps endpoint validation separate from Tower response mapping"
  )]
  fn handle_request(req: &Request<Incoming>) -> Result<String, HandleError> {
    const BAD_METHOD: WrongMethod = WrongMethod {
      allowed: &[Method::GET]
    };
    trace!("handling request...");
    match (req.method(), req.uri().path()) {
      (&Method::GET, "/z") => {
        trace!(error = %"i don't like this letter.", letter = "z");
        Err(HandleError::Unknown)
      }
      (&Method::GET, path) => {
        let character = path
          .as_bytes()
          .get(1)
          .copied()
          .filter(u8::is_ascii)
          .map(char::from)
          .ok_or(HandleError::BadPath)?;
        let content_length_header = req.headers().get(header::CONTENT_LENGTH).ok_or(HandleError::NoContentLength)?;
        trace!(req.content_length = ?content_length_header);
        let content_length = content_length_header
          .to_str()
          .map_err(HandleError::bad_request)?
          .parse::<usize>()
          .map_err(HandleError::bad_request)?;
        let mut body = String::new();
        let span = span!(
            Level::DEBUG,
            "build_rsp",
            rsp.len = content_length,
            rsp.character = %character
        );
        let _enter = span.enter();
        for idx in 0..content_length {
          body.push(character);
          trace!(rsp.body = ?body, rsp.body.idx = idx);
        }
        Ok(body)
      }
      _ => Err(HandleError::bad_request(BAD_METHOD)),
    }
  }
}

/// Errors that can occur while handling a repetition request.
#[derive(Debug)]
enum HandleError {
  /// The request path did not contain an ASCII character after `/`.
  BadPath,
  /// The request did not include a `Content-Length` header.
  NoContentLength,
  /// The request contained malformed data.
  BadRequest(BoxError),
  /// The example intentionally failed a request.
  Unknown,
}

/// Error returned when a request uses an unsupported method.
#[derive(Debug, Clone)]
struct WrongMethod {
  /// Methods accepted by the endpoint.
  allowed: &'static [Method],
}

/// Factory service that creates repetition services for connections.
#[derive(Clone)]
struct MakeSvc;
impl<T> Service<T> for MakeSvc {
  type Response = Svc;
  type Error = BoxError;
  type Future = Ready<Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, _: T) -> Self::Future {
    future::ok(Svc)
  }
}

/// Admin service that replaces the active formatting filter.
struct AdminSvc<S> {
  /// Reload handle for the format subscriber's `EnvFilter`.
  handle: Handle<EnvFilter, S>,
}

impl<S> Clone for AdminSvc<S> {
  fn clone(&self) -> Self {
    Self {
      handle: self.handle.clone(),
    }
  }
}

impl<S> Service<Request<Incoming>> for AdminSvc<S>
where
  S: tracing::Subscriber + 'static,
{
  type Response = Response<ResponseBody>;
  type Error = BoxError;
  type Future = Pin<Box<dyn Future<Output = Result<Response<ResponseBody>, BoxError>> + Send>>;

  fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn call(&mut self, req: Request<Incoming>) -> Self::Future {
    // we need to clone so that the reference to self
    // isn't outlived by the returned future.
    let handle = self.clone();
    let response_future = async move {
      let response = match (req.method(), req.uri().path()) {
        (&Method::PUT, "/filter") => {
          trace!("setting filter");

          let body = req.into_body().collect().await?.to_bytes();
          match handle.set_from(&body) {
            Err(error) => {
              error!(%error, "setting filter failed!");
              rsp(StatusCode::INTERNAL_SERVER_ERROR, error)?
            }
            Ok(()) => rsp(StatusCode::NO_CONTENT, Bytes::new())?,
          }
        }
        _ => rsp(StatusCode::NOT_FOUND, "try `/filter`")?,
      };
      Ok(response)
    };
    Box::pin(response_future)
  }
}

impl<S> AdminSvc<S>
where
  S: tracing::Subscriber + 'static,
{
  /// Parses and applies an `EnvFilter` from a request body.
  fn set_from(&self, bytes: &Bytes) -> Result<(), String> {
    use std::str;
    let body = str::from_utf8(bytes.as_ref()).map_err(|error| error.to_string())?;
    trace!(request.body = ?body);
    let new_filter = body.parse::<EnvFilter>().map_err(|error| error.to_string())?;
    self.handle.reload(new_filter).map_err(|error| error.to_string())
  }
}

/// Builds a boxed HTTP response for the examples' small text bodies.
fn rsp(status: StatusCode, body: impl Into<Bytes>) -> Result<Response<ResponseBody>, BoxError> {
  Response::builder()
    .status(status)
    .body(Full::new(body.into()).boxed())
    .map_err(|error| -> BoxError { Box::new(error) })
}

impl HandleError {
  /// Wraps parser and header conversion failures as bad requests.
  fn bad_request(error: impl Error + Send + Sync + 'static) -> Self {
    Self::BadRequest(Box::new(error))
  }
}

impl fmt::Display for HandleError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::BadPath => formatter.pad("path must be a single ASCII character"),
      Self::NoContentLength => formatter.pad("request must have Content-Length header"),
      Self::BadRequest(ref error) => write!(formatter, "bad request: {error}"),
      Self::Unknown => formatter.pad("unknown internal error"),
    }
  }
}

impl Error for HandleError {}

impl fmt::Display for WrongMethod {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    let allowed = self.allowed.iter().map(Method::as_str).collect::<Vec<_>>().join(", ");
    write!(formatter, "unsupported method: please use one of {allowed}")
  }
}

impl Error for WrongMethod {}

/// Generates a random repetition length and URI for the load generator.
#[allow(
  clippy::single_call_fn,
  reason = "keeps randomized request generation named inside the load generator"
)]
fn gen_uri(authority: &str) -> (usize, String) {
  const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
  let mut rng = rand::rng();
  let letter_index = rng.random_range(0..=ALPHABET.len());
  let request_len = rng.random_range(0..26);
  let letter = ALPHABET
    .as_bytes()
    .get(letter_index)
    .copied()
    .map(char::from)
    .map_or_else(String::new, |character| character.to_string());
  (request_len, format!("http://{authority}/{letter}"))
}

/// Runs one background stream of randomized requests against the server.
#[tracing::instrument(target = "gen", "load_gen")]
async fn load_gen(addr: &SocketAddr) -> Result<(), BoxError> {
  let client: Client<_, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
  let client_service = ServiceBuilder::new()
    .buffer(5)
    .layer(request_span::layer(req_span))
    .timeout(Duration::from_millis(200))
    .service(client);
  let mut interval = time::interval(Duration::from_millis(50));

  loop {
    let _instant = interval.tick().await;
    let authority = addr.to_string();
    let mut ready_service = client_service.clone().ready_oneshot().await?;

    let request_future = async move {
      let sleep = rand::rng().random_range(0..25);
      time::sleep(Duration::from_millis(sleep)).await;

      let (len, uri) = gen_uri(&authority);
      let req = Request::get(uri.as_str())
        .header("Content-Length", len)
        .body(Empty::<Bytes>::new())
        .map_err(|error| -> BoxError { Box::new(error) })?;

      let span = tracing::debug_span!(
          target: "gen",
          "request",
          req.method = ?req.method(),
          req.path = ?req.uri().path(),
      );
      async move {
        info!(target: "gen", "sending request");
        let response = match ready_service.call(req).await {
          Err(error) => {
            error!(target: "gen", error = %error, "request error!");
            return Err(error);
          }
          Ok(response) => response,
        };

        let status = response.status();
        if status != StatusCode::OK {
          error!(target: "gen", status = %status, "error received from server!");
        }

        let body_bytes = match response.into_body().collect().await {
          Err(error) => {
            error!(target: "gen", error = ?error, "body error!");
            return Err(error.into());
          }
          Ok(body) => body.to_bytes(),
        };
        let body_text = String::from_utf8(body_bytes.to_vec())?;
        info!(target: "gen", message = "response complete.", rsp.body = %body_text);
        Ok::<(), BoxError>(())
      }
      .instrument(span)
      .await
    }
    .instrument(info_span!(target: "gen", "generated_request", remote.addr=%addr).or_current());
    let _task = tokio::spawn(request_future);
  }
}

/// Creates the tracing span attached to each client or server request.
fn req_span<A>(req: &Request<A>) -> Span {
  let span = tracing::span!(
      target: "gen",
      Level::INFO,
      "request",
      req.method = ?req.method(),
      req.path = ?req.uri().path(),
  );
  debug!(
      parent: &span,
      message = "received request.",
      req.headers = ?req.headers(),
      req.version = ?req.version(),
  );
  span
}
