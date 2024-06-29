use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::client::conn::http1::Builder;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use hyper::{Method, Request, Response, StatusCode, Uri};

use hyper_util::rt::TokioIo;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as AsyncMutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;

    let backends = vec!["127.0.0.1:8000".to_string(), "127.0.0.1:8080".to_string()];
    let backend_index = Arc::new(AsyncMutex::new(0));

    loop {
        let (stream, _) = listener.accept().await?;
        let backends = backends.clone();
        let backend_index = Arc::clone(&backend_index);

        let io: TokioIo<TcpStream> = TokioIo::new(stream);

        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .preserve_header_case(true)
                .title_case_headers(true)
                .serve_connection(
                    io,
                    service_fn(move |req| {
                        let backends = backends.clone();
                        let backend_index = Arc::clone(&backend_index);
                        proxy(req, backends, backend_index)
                    }),
                )
                .with_upgrades()
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn proxy(
    req: Request<hyper::body::Incoming>,
    backends: Vec<String>,
    backend_index: Arc<AsyncMutex<usize>>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    println!("req: {:?}", req);

    if Method::CONNECT == req.method() {
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            eprintln!("server io error: {}", e);
                        }
                    }
                    Err(e) => eprintln!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(empty()))
        } else {
            eprintln!("CONNECT host is not socket addr: {:?}", req.uri());
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
        // Handle WebSocket upgrade request
        let (host, port) = {
            let mut index = backend_index.lock().await;
            let addr = &backends[*index];
            *index = (*index + 1) % backends.len();
            let parts: Vec<&str> = addr.split(':').collect();
            (parts[0], parts[1].parse::<u16>().unwrap())
        };

        let addr = format!("{}:{}", host, port);
        tokio::task::spawn(async move {
            match hyper::upgrade::on(req).await {
                Ok(upgraded) => {
                    if let Err(e) = tunnel(upgraded, addr).await {
                        eprintln!("server io error: {}", e);
                    }
                }
                Err(e) => eprintln!("upgrade error: {}", e),
            }
        });

        Ok(Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .body(empty())
            .unwrap())
    } else {
        let (host, port) = {
            let mut index = backend_index.lock().await;
            let addr = &backends[*index];
            *index = (*index + 1) % backends.len();
            let parts: Vec<&str> = addr.split(':').collect();
            (parts[0], parts[1].parse::<u16>().unwrap())
        };

        let stream = TcpStream::connect((host, port)).await.unwrap();
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

        let resp = sender.send_request(req).await?;
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

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;
    let mut upgraded = TokioIo::new(upgraded);

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    Ok(())
}
