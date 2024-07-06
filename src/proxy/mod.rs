use rand::Rng;
use std::net::SocketAddr;

use std::sync::{Arc, Mutex};
use tokio::io::{copy_bidirectional, AsyncWriteExt};
use tracing::debug;

use tokio::net::{TcpListener, TcpStream};

pub mod backend;

use backend::Backend;

use crate::configuration::LoadBalancingAlgorithm;
use crate::configuration::Settings;

#[derive(Debug)]
pub struct Server {
    pub listen_addr: SocketAddr,
    pub backends: Vec<Backend>,
    pub current_index: Mutex<usize>,
    pub selector: LoadBalancingAlgorithm,
}

impl Server {
    pub fn new(config: Settings) -> Arc<Self> {
        // let rcvr = config.clone().watch_config().unwrap();

        let new_server = Self {
            listen_addr: config.listen_addr,
            backends: config.backends,
            current_index: Mutex::new(0),
            selector: config.algorithm,
        };
        Arc::new(new_server)
    }
    pub fn update(&mut self, settings: Settings) {
        self.backends = settings.backends;

        self.selector = settings.algorithm;
    }

    pub async fn get_next(&self) -> SocketAddr {
        match self.selector {
            LoadBalancingAlgorithm::Random => {
                let mut rng = rand::thread_rng();
                let random_index = rng.gen_range(0..self.backends.len());
                tracing::info!("New random number : {}", random_index);
                self.backends[random_index].listen_addr
            }
            LoadBalancingAlgorithm::RoundRobin => {
                let mut index: std::sync::MutexGuard<usize> = self.current_index.lock().unwrap();
                let addr = self.backends[*index].listen_addr;
                *index = (*index + 1) % self.backends.len();
                addr
            }
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("Listening on {}", self.listen_addr);

        loop {
            let (client_stream, _) = listener.accept().await?;
            let backend_addr = self.get_next().await;
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
