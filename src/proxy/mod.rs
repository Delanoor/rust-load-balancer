use bytes::Bytes;
use http::header::CONNECTION;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, info};

pub mod backend;

use backend::Backend;

#[derive(Debug)]
pub struct Server {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Arc<Backend>>,
    pub current_index: Mutex<usize>,
}

impl Server {
    pub fn new(listen_addr: SocketAddr, backends: Vec<Backend>) -> Self {
        let mut new_server = Server {
            listen_addr,
            backends: Vec::new(),
            current_index: Mutex::new(0),
        };
        let mut new_backends = Vec::new();

        for backend in backends.into_iter() {
            new_backends.push(Arc::new(backend))
        }

        new_server.backends = new_backends.to_owned();
        new_server
    }

    pub fn get_next(&self) -> SocketAddr {
        let mut index = self.current_index.lock().unwrap();
        let addr = self.backends[*index].listen_addr;
        *index = (*index + 1) % self.backends.len();
        addr
    }

    pub async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.listen_addr).await?;

        tracing::info!("Listening on {}", self.listen_addr);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let service = Arc::clone(&self);
                    tokio::task::spawn(async move {
                        if let Err(err) = http1::Builder::new()
                            .preserve_header_case(true)
                            .title_case_headers(true)
                            .serve_connection(
                                TokioIo::new(stream),
                                service_fn(move |req| proxy(req, service.clone())),
                            )
                            .with_upgrades()
                            .await
                        {
                            eprintln!("Error serving connection: {:?}", err);
                        }
                    });
                }
                Err(err) => {
                    eprintln!("Error accepting connection: {:?}", err);
                }
            }
        }
    }
}

async fn proxy(
    req: Request<hyper::body::Incoming>,
    server: Arc<Server>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    let backend_addr = server.get_next().to_string();

    debug!("Received req: {:?}", req);

    tracing::info!("Next backend: {}", backend_addr);

    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();

    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            tracing::error!("Server IO error: {}", e);
                        }
                    }
                    Err(e) => tracing::error!("Upgrade error: {}", e),
                }
            });

            Ok(Response::new(empty()))
        } else {
            tracing::error!("CONNECT host is not a socket addr: {:?}", req.uri());
            let mut resp = Response::new(full("CONNECT must be to a socket address"));
            *resp.status_mut() = StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else if req
        .headers()
        .get("upgrade")
        .map(|v| v == "websocket")
        .unwrap_or(false)
    {
        tokio::task::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = tunnel(upgraded, backend_addr).await {
                        tracing::error!("Upgrade IO error: {}", e);
                    }
                }
                Err(e) => tracing::error!("Upgrade error: {}", e),
            }
        });

        Ok(Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .body(empty())
            .unwrap())
    } else {
        let stream = TcpStream::connect(&backend_addr)
            .await
            .expect("connection error");
        let io = TokioIo::new(stream);

        let (mut sender, conn) = Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .handshake(io)
            .await?;
        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                tracing::error!("Connection failed: {:?}", err);
            }
        });

        // Forward the request and get the response
        let resp = sender.send_request(req).await.map_err(|e| {
            tracing::error!("Send request error: {:?}", e);
            e
        })?;

        let mut response = resp.map(|b| b.boxed());

        if headers
            .get(CONNECTION)
            .map(|v| v == "close")
            .unwrap_or(false)
        {
            response
                .headers_mut()
                .insert(CONNECTION, "close".parse().unwrap());
        }
        Ok(response)
    }
}

fn host_addr(uri: &Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;
    let mut upgraded = TokioIo::new(upgraded);

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    println!(
        "Client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}
