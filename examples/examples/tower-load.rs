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

use bytes::Bytes;
use futures::{
    Future,
    future::{self, Ready},
};
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use rand::RngExt;
use std::{
    convert::Infallible,
    error::Error,
    fmt,
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::net::TcpListener;
use tokio::{time, try_join};
use tower::{Service, ServiceBuilder, ServiceExt};
use tracing::{
    self, Instrument as _, Level, Span, debug, error, info, info_span, span, trace, warn,
};
use tracing_subscriber::{filter::EnvFilter, reload::Handle};
use tracing_tower::{GetSpan, request_span, request_span::make};

type Err = Box<dyn Error + Send + Sync + 'static>;
type RspBody = BoxBody<Bytes, Infallible>;

#[tokio::main]
async fn main() -> Result<(), Err> {
    let builder = tracing_subscriber::fmt()
        .with_env_filter("info,tower_load=debug")
        .with_filter_reloading();
    let handle = builder.reload_handle();
    builder.try_init()?;

    let addr = "[::1]:3000".parse::<SocketAddr>()?;
    let admin_addr = "[::1]:3001".parse::<SocketAddr>()?;

    let admin = AdminSvc { handle };

    let make_svc = ServiceBuilder::new()
        .layer(make::layer::<_, Svc, _>(req_span))
        .service(MakeSvc);

    let res = try_join!(
        tokio::spawn(load_gen(addr)),
        tokio::spawn(load_gen(addr)),
        tokio::spawn(load_gen(addr)),
        tokio::spawn(serve(addr, make_svc)),
        tokio::spawn(serve_admin(admin_addr, admin)),
    );

    match res {
        Ok(_) => info!("load generator exited successfully"),
        Err(e) => {
            error!(error = ?e, "load generator failed");
        }
    }
    Ok(())
}

async fn serve<G>(
    addr: SocketAddr,
    mut make_svc: make::MakeService<MakeSvc, Request<Incoming>, G>,
) -> Result<(), Err>
where
    G: GetSpan<Request<Incoming>> + Clone + Send + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let svc = make_svc.call(remote_addr).await?;
        let hyper_svc = TowerToHyperService::new(svc);
        tokio::spawn(async move {
            if let Err(e) = auto::Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_svc)
                .await
            {
                error!(error = %e, "connection error");
            }
        });
    }
}

async fn serve_admin<S>(addr: SocketAddr, admin: AdminSvc<S>) -> Result<(), Err>
where
    S: tracing::Subscriber + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _remote_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let hyper_svc = TowerToHyperService::new(admin.clone());
        tokio::spawn(async move {
            if let Err(e) = auto::Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_svc)
                .await
            {
                error!(error = %e, "admin connection error");
            }
        });
    }
}

#[derive(Clone)]
struct Svc;
impl Service<Request<Incoming>> for Svc {
    type Response = Response<RspBody>;
    type Error = Err;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let rsp = Self::handle_request(req)
            .map(|body| {
                trace!("sending response");
                rsp(StatusCode::OK, body)
            })
            .unwrap_or_else(|e| {
                trace!(rsp.error = %e);
                let status = match e {
                    HandleError::BadPath => {
                        warn!(rsp.status = ?StatusCode::NOT_FOUND);
                        StatusCode::NOT_FOUND
                    }
                    HandleError::NoContentLength | HandleError::BadRequest(_) => {
                        StatusCode::BAD_REQUEST
                    }
                    HandleError::Unknown => StatusCode::INTERNAL_SERVER_ERROR,
                };
                rsp(status, e.to_string())
            });
        future::ok(rsp)
    }
}

impl Svc {
    fn handle_request(req: Request<Incoming>) -> Result<String, HandleError> {
        const BAD_METHOD: WrongMethod = WrongMethod(&[Method::GET]);
        trace!("handling request...");
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/z") => {
                trace!(error = %"i don't like this letter.", letter = "z");
                Err(HandleError::Unknown)
            }
            (&Method::GET, path) => {
                let ch = path.get(1..2).ok_or(HandleError::BadPath)?;
                let content_length = req
                    .headers()
                    .get(header::CONTENT_LENGTH)
                    .ok_or(HandleError::NoContentLength)?;
                trace!(req.content_length = ?content_length);
                let content_length = content_length
                    .to_str()
                    .map_err(HandleError::bad_request)?
                    .parse::<usize>()
                    .map_err(HandleError::bad_request)?;
                let mut body = String::new();
                let span = span!(
                    Level::DEBUG,
                    "build_rsp",
                    rsp.len = content_length,
                    rsp.character = ch
                );
                let _enter = span.enter();
                for idx in 0..content_length {
                    body.push_str(ch);
                    trace!(rsp.body = ?body, rsp.body.idx = idx);
                }
                Ok(body)
            }
            _ => Err(HandleError::bad_request(BAD_METHOD)),
        }
    }
}

#[derive(Debug)]
enum HandleError {
    BadPath,
    NoContentLength,
    BadRequest(Box<dyn Error + Send + 'static>),
    Unknown,
}

#[derive(Debug, Clone)]
struct WrongMethod(&'static [Method]);

#[derive(Clone)]
struct MakeSvc;
impl<T> Service<T> for MakeSvc {
    type Response = Svc;
    type Error = Err;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        future::ok(Svc)
    }
}

struct AdminSvc<S> {
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
    type Response = Response<RspBody>;
    type Error = Err;
    type Future = Pin<Box<dyn Future<Output = Result<Response<RspBody>, Err>> + std::marker::Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        // we need to clone so that the reference to self
        // isn't outlived by the returned future.
        let handle = self.clone();
        let f = async move {
            let rsp = match (req.method(), req.uri().path()) {
                (&Method::PUT, "/filter") => {
                    trace!("setting filter");

                    let body = req.into_body().collect().await?.to_bytes();
                    match handle.set_from(body) {
                        Err(error) => {
                            error!(%error, "setting filter failed!");
                            rsp(StatusCode::INTERNAL_SERVER_ERROR, error)
                        }
                        Ok(()) => rsp(StatusCode::NO_CONTENT, empty()),
                    }
                }
                _ => rsp(StatusCode::NOT_FOUND, "try `/filter`"),
            };
            Ok(rsp)
        };
        Box::pin(f)
    }
}

impl<S> AdminSvc<S>
where
    S: tracing::Subscriber + 'static,
{
    fn set_from(&self, bytes: Bytes) -> Result<(), String> {
        use std::str;
        let body = str::from_utf8(bytes.as_ref()).map_err(|e| format!("{}", e))?;
        trace!(request.body = ?body);
        let new_filter = body
            .parse::<tracing_subscriber::filter::EnvFilter>()
            .map_err(|e| format!("{}", e))?;
        self.handle.reload(new_filter).map_err(|e| format!("{}", e))
    }
}

fn rsp(status: StatusCode, body: impl Into<Bytes>) -> Response<RspBody> {
    Response::builder()
        .status(status)
        .body(Full::new(body.into()).boxed())
        .expect("builder with known status code must not fail")
}

fn empty() -> Bytes {
    Bytes::new()
}

impl HandleError {
    fn bad_request(e: impl std::error::Error + Send + 'static) -> Self {
        HandleError::BadRequest(Box::new(e))
    }
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandleError::BadPath => f.pad("path must be a single ASCII character"),
            HandleError::NoContentLength => f.pad("request must have Content-Length header"),
            HandleError::BadRequest(e) => write!(f, "bad request: {}", e),
            HandleError::Unknown => f.pad("unknown internal error"),
        }
    }
}

impl std::error::Error for HandleError {}

impl fmt::Display for WrongMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported method: please use one of {:?}", self.0)
    }
}

impl std::error::Error for WrongMethod {}

fn gen_uri(authority: &str) -> (usize, String) {
    static ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut rng = rand::rng();
    let idx = rng.random_range(0..ALPHABET.len() + 1);
    let len = rng.random_range(0..26);
    let letter = ALPHABET.get(idx..=idx).unwrap_or("");
    (len, format!("http://{}/{}", authority, letter))
}

#[tracing::instrument(target = "gen", "load_gen")]
async fn load_gen(addr: SocketAddr) -> Result<(), Err> {
    let client: Client<_, Empty<Bytes>> = Client::builder(TokioExecutor::new()).build_http();
    let svc = ServiceBuilder::new()
        .buffer(5)
        .layer(request_span::layer(req_span))
        .timeout(Duration::from_millis(200))
        .service(client);
    let mut interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        interval.tick().await;
        let authority = format!("{}", addr);
        let mut svc = svc.clone().ready_oneshot().await?;

        let f = async move {
            let sleep = rand::rng().random_range(0..25);
            time::sleep(Duration::from_millis(sleep)).await;

            let (len, uri) = gen_uri(&authority);
            let req = Request::get(&uri[..])
                .header("Content-Length", len)
                .body(Empty::<Bytes>::new())
                .unwrap();

            let span = tracing::debug_span!(
                target: "gen",
                "request",
                req.method = ?req.method(),
                req.path = ?req.uri().path(),
            );
            async move {
                info!(target: "gen", "sending request");
                let rsp = match svc.call(req).await {
                    Err(e) => {
                        error!(target: "gen", error = %e, "request error!");
                        return Err(e);
                    }
                    Ok(rsp) => rsp,
                };

                let status = rsp.status();
                if status != StatusCode::OK {
                    error!(target: "gen", status = ?status, "error received from server!");
                }

                let body = match rsp.into_body().collect().await {
                    Err(e) => {
                        error!(target: "gen", error = ?e, "body error!");
                        return Err(e.into());
                    }
                    Ok(body) => body.to_bytes(),
                };
                let body = String::from_utf8(body.to_vec())?;
                info!(target: "gen", message = "response complete.", rsp.body = %body);
                Ok(())
            }
            .instrument(span)
            .await
        }
        .instrument(info_span!(target: "gen", "generated_request", remote.addr=%addr).or_current());
        tokio::spawn(f);
    }
}

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
