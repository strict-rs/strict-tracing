use bytes::Bytes;
use futures::future;
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use std::convert::Infallible;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::TcpListener;
use tower::{Service, ServiceBuilder};
use tracing::dispatcher;
use tracing::info;
use tracing_tower::request_span::make;

type Err = Box<dyn std::error::Error + Send + Sync + 'static>;

fn req_span<A>(req: &Request<A>) -> tracing::Span {
    let span = tracing::info_span!(
        "request",
        req.method = ?req.method(),
        req.uri = ?req.uri(),
        req.version = ?req.version(),
        req.headers = ?req.headers()
    );
    tracing::info!(parent: &span, "received request");
    span
}

const ROOT: &str = "/";

#[derive(Debug, Clone)]
pub struct Svc;

impl Service<Request<Incoming>> for Svc {
    type Response = Response<Full<Bytes>>;
    type Error = Infallible;
    type Future = future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Ok(()).into()
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let rsp = Response::builder();

        let uri = req.uri();
        let rsp = if uri.path() != ROOT {
            let body = Full::new(Bytes::new());
            rsp.status(404).body(body).unwrap()
        } else {
            let body = Full::new(Bytes::from_static(b"heyo!"));
            rsp.status(200).body(body).unwrap()
        };
        let span = tracing::info_span!(
            "response",
            rsp.status = ?rsp.status(),
            rsp.version = ?rsp.version(),
            rsp.headers = ?rsp.headers()
        );

        dispatcher::get_default(|dispatch| {
            let id = span.id().expect("Missing ID; this is a bug");
            if let Some(current) = dispatch.current_span().id() {
                dispatch.record_follows_from(&id, current)
            }
        });
        let _guard = span.enter();
        info!("sending response");
        future::ok(rsp)
    }
}

pub struct MakeSvc;

impl<T> Service<T> for MakeSvc {
    type Response = Svc;
    type Error = std::io::Error;
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
    tracing_subscriber::fmt()
        .with_env_filter("tower=trace")
        .try_init()?;

    let mut make_svc = ServiceBuilder::new()
        .timeout(Duration::from_millis(250))
        .layer(make::layer::<_, Svc, _>(req_span))
        .service(MakeSvc);

    let addr: std::net::SocketAddr = "127.0.0.1:3000".parse()?;
    let listener = TcpListener::bind(addr).await?;
    info!(message = "listening", addr = ?addr);

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
                tracing::error!(error = %e, "connection error");
            }
        });
    }
}
