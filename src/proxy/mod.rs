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

        println!("Listening on {}", self.listen_addr);

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
                    if let Err(e) = tunnel(upgraded, backend_addr).await {
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
