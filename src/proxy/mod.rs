use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};

use crate::common::types::Backend;

use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct Server {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
    current_index: Arc<Mutex<usize>>,
}

impl Server {
    pub fn new(listen_addr: SocketAddr, backends: Vec<Backend>) -> Self {
        Self {
            listen_addr,
            backends,
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    pub fn get_next_backend(&self) -> SocketAddr {
        let mut index = self.current_index.lock().unwrap();
        let addr = self.backends[*index].addr;
        *index = (*index + 1) % self.backends.len();
        addr
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.listen_addr).await?;

        eprintln!("Listening on {}", self.listen_addr);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let service = self.clone();
                    tokio::task::spawn(async move {
                        if let Err(err) = http1::Builder::new()
                            .preserve_header_case(true)
                            .title_case_headers(true)
                            .serve_connection(
                                TokioIo::new(stream),
                                service_fn(move |req| {
                                    let backend_addr = service.get_next_backend();
                                    proxy(req, backend_addr)
                                }),
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
    backend_addr: SocketAddr,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            eprintln!("Server IO error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("Upgrade error: {}", e),
                }
            });

            Ok(Response::new(empty()))
        } else {
            eprintln!("CONNECT host is not a socket addr: {:?}", req.uri());
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
                    if let Err(e) = tunnel(upgraded, backend_addr.to_string()).await {
                        eprintln!("Upgrade IO error: {}", e);
                    }
                }
                Err(e) => eprintln!("Upgrade error: {}", e),
            }
        });

        Ok(Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .body(empty())
            .unwrap())
    } else {
        let stream = TcpStream::connect(backend_addr)
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
                println!("Connection failed: {:?}", err);
            }
        });

        // Forward the request and get the response
        let resp = sender.send_request(req).await?;
        println!("Received response from backend: {:?}", resp);

        Ok(resp.map(|b| b.boxed()))
    }
}

fn host_addr(uri: &Uri) -> Option<String> {
    uri.authority().and_then(|auth| Some(auth.to_string()))
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
