use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tracing::debug;

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
    pub fn new(listen_addr: SocketAddr, backends: Vec<Backend>) -> Arc<Self> {
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
        Arc::new(new_server)
    }

    pub fn get_next(&self) -> SocketAddr {
        let mut index: std::sync::MutexGuard<usize> = self.current_index.lock().unwrap();
        let addr = self.backends[*index].listen_addr;
        *index = (*index + 1) % self.backends.len();
        addr
    }

    pub async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("Listening on {}", self.listen_addr);

        loop {
            let (client_stream, _) = listener.accept().await?;
            let backend_addr = self.get_next();
            tracing::info!("Forwarding connection to {}", backend_addr);

            let service: Arc<Server> = Arc::clone(&self);
            tokio::task::spawn(handle_connection(client_stream, backend_addr, service));
        }
    }
}

async fn handle_connection(
    mut client_stream: TcpStream,
    backend_addr: SocketAddr,
    _load_balancer: Arc<Server>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut backend_stream = TcpStream::connect(backend_addr).await?;
    match copy_bidirectional(&mut client_stream, &mut backend_stream).await {
        Ok((from_client, from_server)) => {
            debug!(
                "Client wrote {} bytes and received {} bytes",
                from_client, from_server
            );
        }
        Err(e) => {
            eprintln!("Error in connection: {}", e);
        }
    }

    client_stream.shutdown().await?;
    backend_stream.shutdown().await?;

    Ok(())
}
